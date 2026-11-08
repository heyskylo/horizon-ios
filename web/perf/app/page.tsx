import { redirect } from "next/navigation";

// The UI lives at `/perf` (docs/perf-platform.md §7.1: the address the game prints, `PERF_UI_URL`,
// ends in `/perf`, and the header rule and the copy into playhorizon.dev are both scoped to it).
// The root only sends a visitor there, so opening the app's own address works too.
export default function Home() {
  redirect("/perf");
}
