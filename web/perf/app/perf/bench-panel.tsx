"use client";

import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { abMetrics, metricUnit, ordinals, phaseLabel, signed, spreads, stamp, verdictTone } from "@/lib/bench";
import { grouped, num } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { BenchAb, BenchRow } from "@/lib/types";
import { useRows } from "./use-capture";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

function Frames({ row, title }: { row: BenchRow; title: string }) {
  const places = ordinals(row.frames);
  const bars = spreads(row.frames);
  const hasCpu = row.frames.some((f) => f.cpu_p50_ms != null);
  const hasGpu = row.frames.some((f) => f.gpu_p50_ms != null);
  return (
    <section>
      <h4 className={styles.benchHeading}>{t("bench.frames")}</h4>
      <p className={styles.cardNote}>{t("bench.framesNote")}</p>
      <div className={styles.tableScroll} role="region" tabIndex={0} aria-label={t("bench.region", { scenario: title })}>
        <table className={`${styles.table} ${styles.benchTable}`}>
          <thead>
            <tr>
              <th>{t("bench.phase")}</th>
              <th>{t("bench.bar")}</th>
              <th>{t("bench.frameCount")}</th>
              <th>{t("bench.fps")}</th>
              <th>{t("bench.low1")}</th>
              <th>{t("bench.median")}</th>
              <th>{t("bench.p95")}</th>
              <th>{t("bench.p99")}</th>
              <th>{t("bench.max")}</th>
              {hasCpu ? <th>{t("bench.cpu")}</th> : null}
              {hasGpu ? <th>{t("bench.gpu")}</th> : null}
            </tr>
          </thead>
          <tbody>
            {bars.map(({ frames: f, typical, tail }, index) => {
              const { n, total } = places[index];
              const name = phaseLabel(f.phase, row.ab);
              return (
                <tr key={`${f.phase}:${f.repeat}`}>
                  <td className={styles.benchPhase}>{total > 1 ? t("bench.repeat", { phase: name, n, total }) : name}</td>
                  <td>
                    <span className={styles.benchTrack} aria-hidden="true">
                      <span className={styles.benchTail} style={{ width: `${tail * 100}%` }} />
                      <span className={styles.benchTypical} style={{ width: `${typical * 100}%` }} />
                    </span>
                  </td>
                  <td>{grouped(f.frames)}</td>
                  <td>{num(f.avg_fps, 1)}</td>
                  <td>{num(f.low_1pct_fps, 1)}</td>
                  <td>{num(f.p50_ms, 2)} ms</td>
                  <td>{num(f.p95_ms, 2)} ms</td>
                  <td>{num(f.p99_ms, 2)} ms</td>
                  <td>{num(f.max_ms, 2)} ms</td>
                  {hasCpu ? <td>{num(f.cpu_p50_ms, 2)} ms</td> : null}
                  {hasGpu ? <td>{num(f.gpu_p50_ms, 2)} ms</td> : null}
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function Metrics({ row }: { row: BenchRow }) {
  const metrics = Object.entries(row.metrics);
  return (
    <section>
      <h4 className={styles.benchHeading}>{t("bench.metrics")}</h4>
      <p className={styles.cardNote}>{t("bench.metricsNote")}</p>
      <div className={styles.tableScroll}>
        <table className={`${styles.table} ${styles.benchMetrics}`}>
          <thead>
            <tr>
              <th>{t("bench.metric")}</th>
              <th>{t("bench.runs")}</th>
              <th>{t("bench.mean")}</th>
              <th>{t("bench.ci")}</th>
              <th>{t("bench.range")}</th>
            </tr>
          </thead>
          <tbody>
            {metrics.map(([name, m]) => {
              const unit = metricUnit(name);
              const withUnit = (value: number) => (unit ? `${num(value, 2)} ${unit}` : num(value, 2));
              return (
                <tr key={name}>
                  <td className={styles.benchPhase} title={name}>
                    {name}
                  </td>
                  <td>{m.n}</td>
                  <td>{withUnit(m.mean)}</td>
                  <td>{m.ci95 == null ? "—" : num(m.ci95, 2)}</td>
                  <td>
                    {num(m.min, 2)} – {withUnit(m.max)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function Paired({ ab }: { ab: BenchAb }) {
  return (
    <section>
      <h4 className={styles.benchHeading}>{t("bench.ab", { switch: ab.switch })}</h4>
      <p className={styles.cardNote}>
        {t("bench.abNote", { pairs: ab.pairs, seconds: num(ab.block_s, 0), a: ab.a, b: ab.b })}
      </p>
      {ab.first_block_outside_range ? <p className={styles.cardNote}>{t("bench.abFirstBlock")}</p> : null}
      <div className={styles.tableScroll}>
        <table className={`${styles.table} ${styles.benchMetrics}`}>
          <thead>
            <tr>
              <th>{t("bench.abMetric")}</th>
              <th>{phaseLabel("ab.a", ab)}</th>
              <th>{phaseLabel("ab.b", ab)}</th>
              <th>{t("bench.abChange")}</th>
              <th>{t("bench.abInterval")}</th>
              <th>{t("bench.abVerdict")}</th>
            </tr>
          </thead>
          <tbody>
            {abMetrics(ab.result).map(([name, p]) => (
              <tr key={name}>
                <td className={styles.benchPhase}>{t(`bench.abMetric.${name}`)}</td>
                <td>{num(p.a, 2)} ms</td>
                <td>{num(p.b, 2)} ms</td>
                <td>
                  {signed(p.diff, 2)} ms{p.diff_pct == null ? "" : ` (${signed(p.diff_pct, 1)}%)`}
                </td>
                <td>{p.ci95 ? t("bench.abRange", { low: signed(p.ci95[0], 2), high: signed(p.ci95[1], 2) }) : "—"}</td>
                <td className={styles.verdictCell} data-tone={verdictTone(p.verdict)}>
                  {t(`bench.verdict.${p.verdict}`)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

export function BenchPanel({ reader, capture }: Props) {
  const { rows, error } = useRows<BenchRow>(reader, capture, "bench");

  if (error) return <p className={styles.error}>{error}</p>;
  if (rows.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
  return (
    <div className={styles.summary}>
      {rows.map((row) => {
        const title = row.scenario ?? t("bench.untitled");
        const readable = row.frames.length > 0 || Object.keys(row.metrics).length > 0;
        const meta = [
          row.app_version ? t("bench.version", { version: row.app_version }) : null,
          row.created ? stamp(row.created) : null,
        ].filter((part) => part !== null);
        return (
          <article key={row.id} className={styles.benchReport}>
            <header>
              <h3 className={styles.cardTitle}>{title}</h3>
              {meta.length ? <p className={styles.cardNote}>{meta.join(" · ")}</p> : null}
            </header>
            {readable ? null : <p className={styles.empty}>{t("bench.unreadable")}</p>}
            {row.ab ? <Paired ab={row.ab} /> : null}
            {row.frames.length ? <Frames row={row} title={title} /> : null}
            {Object.keys(row.metrics).length ? <Metrics row={row} /> : null}
          </article>
        );
      })}
    </div>
  );
}
