"""Start a process so that everything it starts ends with it: harnesses and the games, servers and probes they start.

A harness that crashes, is killed or times out used to leave its games running: one held the GPU for three hours
(2026-11-09, #13751). Each tool that starts a game can't be trusted to end it, so the runner ends them all:

- POSIX: the process leads its own session (process group). When it exits, or is killed, the rest of the group
  goes too. The group is killed while the exited leader is still unreaped, so its id can't name another group
  (os.waitid: macOS has it from Python 3.13; before, only a kill ends the group). A process that starts its own
  session (start_new_session) leaves the group, and ends only if it ends itself.
- Windows: the process starts suspended, joins its own Job Object, then resumes, so whatever it starts joins the
  job too. When it exits, or is killed, the job ends the rest. The job is closed on exit
  (JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE), so a runner that dies takes its harnesses' processes with it. A process
  started with CREATE_BREAKAWAY_FROM_JOB (a build cache's server, say) may leave the job, as such a process
  asks to outlive whoever started it.

Only the process and what it started are ever killed: nothing is found by name, and a process started elsewhere
(a player's game, another runner's harness) is never in the group or job.
"""

import os
import signal
import subprocess

if os.name == "nt":
    import ctypes
    from ctypes import wintypes

    CREATE_SUSPENDED = 0x4
    JOB_OBJECT_LIMIT_BREAKAWAY_OK = 0x800
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x2000
    JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS = 9
    PROCESS_SET_QUOTA, PROCESS_TERMINATE = 0x100, 0x1
    TH32CS_SNAPTHREAD, THREAD_SUSPEND_RESUME = 0x4, 0x2
    INVALID_HANDLE_VALUE = wintypes.HANDLE(-1).value

    class IO_COUNTERS(ctypes.Structure):
        _fields_ = [(name, ctypes.c_ulonglong) for name in
                    ("ReadOperationCount", "WriteOperationCount", "OtherOperationCount",
                     "ReadTransferCount", "WriteTransferCount", "OtherTransferCount")]

    class JOBOBJECT_BASIC_LIMIT_INFORMATION(ctypes.Structure):
        _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64),
                    ("LimitFlags", wintypes.DWORD), ("MinimumWorkingSetSize", ctypes.c_size_t),
                    ("MaximumWorkingSetSize", ctypes.c_size_t), ("ActiveProcessLimit", wintypes.DWORD),
                    ("Affinity", ctypes.c_size_t), ("PriorityClass", wintypes.DWORD),
                    ("SchedulingClass", wintypes.DWORD)]

    class JOBOBJECT_EXTENDED_LIMIT_INFORMATION(ctypes.Structure):
        _fields_ = [("BasicLimitInformation", JOBOBJECT_BASIC_LIMIT_INFORMATION), ("IoInfo", IO_COUNTERS),
                    ("ProcessMemoryLimit", ctypes.c_size_t), ("JobMemoryLimit", ctypes.c_size_t),
                    ("PeakProcessMemoryUsed", ctypes.c_size_t), ("PeakJobMemoryUsed", ctypes.c_size_t)]

    class THREADENTRY32(ctypes.Structure):
        _fields_ = [("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD), ("th32ThreadID", wintypes.DWORD),
                    ("th32OwnerProcessID", wintypes.DWORD), ("tpBasePri", wintypes.LONG),
                    ("tpDeltaPri", wintypes.LONG), ("dwFlags", wintypes.DWORD)]

    kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel32.CreateJobObjectW.restype = wintypes.HANDLE
    kernel32.CreateJobObjectW.argtypes = [wintypes.LPVOID, wintypes.LPCWSTR]
    kernel32.SetInformationJobObject.argtypes = [wintypes.HANDLE, ctypes.c_int, wintypes.LPVOID, wintypes.DWORD]
    kernel32.OpenProcess.restype = wintypes.HANDLE
    kernel32.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel32.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
    kernel32.TerminateJobObject.argtypes = [wintypes.HANDLE, wintypes.UINT]
    kernel32.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    kernel32.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
    kernel32.Thread32First.argtypes = [wintypes.HANDLE, ctypes.POINTER(THREADENTRY32)]
    kernel32.Thread32Next.argtypes = [wintypes.HANDLE, ctypes.POINTER(THREADENTRY32)]
    kernel32.OpenThread.restype = wintypes.HANDLE
    kernel32.OpenThread.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel32.ResumeThread.restype = wintypes.DWORD
    kernel32.ResumeThread.argtypes = [wintypes.HANDLE]

    def _job():
        """A new job that kills its processes when its last handle closes; None if Windows refuses one."""
        job = kernel32.CreateJobObjectW(None, None)
        if not job:
            return None
        info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION()
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK
        if not kernel32.SetInformationJobObject(job, JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
                                                ctypes.byref(info), ctypes.sizeof(info)):
            kernel32.CloseHandle(job)
            return None
        return job

    def _assign(job, pid):
        process = kernel32.OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, False, pid)
        if not process:
            return False
        try:
            return bool(kernel32.AssignProcessToJobObject(job, process))
        finally:
            kernel32.CloseHandle(process)

    def _resume(pid):
        """Resume the threads of a process started with CREATE_SUSPENDED (only its first one exists)."""
        snapshot = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
        if snapshot == INVALID_HANDLE_VALUE:
            raise ctypes.WinError(ctypes.get_last_error())
        try:
            entry = THREADENTRY32()
            entry.dwSize = ctypes.sizeof(entry)
            more = kernel32.Thread32First(snapshot, ctypes.byref(entry))
            while more:
                if entry.th32OwnerProcessID == pid:
                    thread = kernel32.OpenThread(THREAD_SUSPEND_RESUME, False, entry.th32ThreadID)
                    if not thread:
                        raise ctypes.WinError(ctypes.get_last_error())
                    try:
                        kernel32.ResumeThread(thread)
                    finally:
                        kernel32.CloseHandle(thread)
                more = kernel32.Thread32Next(snapshot, ctypes.byref(entry))
        finally:
            kernel32.CloseHandle(snapshot)


def popen(arguments, **options):
    """subprocess.Popen(arguments, **options), with everything the process starts tied to it (see above)."""
    if os.name != "nt":
        return subprocess.Popen(arguments, start_new_session=True, **options)
    job = _job()
    if job is None:
        return subprocess.Popen(arguments, **options)
    process = subprocess.Popen(arguments, creationflags=options.pop("creationflags", 0) | CREATE_SUSPENDED, **options)
    try:
        if _assign(job, process.pid):
            process.job = job
        else:
            kernel32.CloseHandle(job)  # left without a job: kill() falls back to its tree
    finally:
        try:
            _resume(process.pid)
        except OSError:
            kill(process)
            process.wait()
            ended(process)
            raise
    return process


def kill(process):
    """Kill the process and everything it started, at once. A process already reaped (returncode set) is left
    alone on POSIX: its group's id may name another group by now."""
    if os.name != "nt":
        if process.returncode is not None:
            return
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass  # the group is gone (a zombie leader alone can answer EPERM on macOS)
        return
    job = getattr(process, "job", None)
    if job:
        kernel32.TerminateJobObject(job, 1)
    elif process.poll() is None:
        subprocess.run(["taskkill", "/T", "/F", "/PID", str(process.pid)], capture_output=True)


def ended(process):
    """Whether the process has exited (like poll()); once it has, what it left running is killed."""
    if process.returncode is not None:
        _release(process)
        return True
    if os.name != "nt":
        if not hasattr(os, "waitid"):  # macOS before Python 3.13: without it, what the process left keeps running
            return process.poll() is not None
        # Kill the rest of the group before reaping the leader: its zombie keeps the group's id from being reused.
        try:
            exited = os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT)
        except OSError:  # reaped elsewhere already: its group's id is no longer safe to use
            return process.poll() is not None
        if not exited:
            return False
        kill(process)
        process.wait()
        return True
    if process.poll() is None:
        return False
    _release(process)
    return True


def _release(process):
    job = getattr(process, "job", None)
    if job:
        process.job = None
        kernel32.TerminateJobObject(job, 1)
        kernel32.CloseHandle(job)
