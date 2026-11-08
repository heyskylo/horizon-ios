import { listCaptureMetas, deleteCapture } from "./storage";

let started = false;

export function ensureSweep(): void {
  if (started) return;
  started = true;
  const run = async () => {
    try {
      const metas = await listCaptureMetas();
      const now = Date.now();
      for (const m of metas) {
        if (new Date(m.expires_utc).getTime() <= now) {
          await deleteCapture(m.id);
        }
      }
    } catch {
      // ignore
    }
  };
  run();
  setInterval(run, 300_000);
}
