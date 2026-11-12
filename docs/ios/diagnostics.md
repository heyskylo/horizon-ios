# Mobile diagnostics

Launcher Settings → Logs → Export logs opens the system file-save picker with a UTF-8 text report. Reproduce a failure first, then return to the launcher and export. Exporting does not upload anything.

The iOS writer retains `latest.log` and `previous.log`, each at most 1 MiB. Launcher diagnostics use two more 1 MiB files. The reusable export is at most 4 MiB, so retained logs plus one export consume at most 8 MiB, excluding short-lived atomic-write staging. iOS removes the old date-based gzip log archives when adopting this bounded policy. Desktop logging is unchanged.

Game logs include the Rust Minecraft implementation, renderer, shader errors and integrated-server events that use the game logger. There is no separate Java process/log in this port. Raw OS console output and OS crash reports are not included. Shader loading already records the failure reason when a pack is disabled; the report is intended to diagnose that failure, not claim it is repaired.

Authentication-bearing messages are redacted before writing. Export applies another filter for authentication markers, URLs, emails, IP addresses, account UUIDs and long opaque strings. This is a defensive filter, not a guarantee against every possible secret format. Review a report before sharing: chat, world and server names may remain. Account databases, keychains, options, worlds, and resource packs are never collected.

Checks exercise real Rust rollover and redaction, Swift bounded storage/export, and the simulator Settings → Export → native file-save interaction. These checks do not reproduce a physical-device shader failure.
