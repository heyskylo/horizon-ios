import type { Metadata } from "next";
import type { ReactNode } from "react";

export const metadata: Metadata = {
  title: "Horizon performance captures",
  icons: "/perf/horizon-64.png",
  description:
    "Read a .hzperf capture in your browser with the same reader as horizon-perf: health, metadata, call tree and comparisons.",
};

// The app's only global rule is the page box; everything else is scoped in page.module.css
// (docs/perf-platform.md §7.1: CSS modules, no global CSS).
export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="en">
      <body style={{ margin: 0, background: "#0d0e13", color: "#ecedf3" }}>{children}</body>
    </html>
  );
}
