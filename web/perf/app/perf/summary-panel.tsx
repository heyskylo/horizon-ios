"use client";

import { useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { at, grouped, mib, num, shortId, withUnit } from "@/lib/format";
import { byField, stats } from "@/lib/health";
import type { Stats } from "@/lib/health";
import { t } from "@/lib/i18n";
import {
  attributed,
  flatPhases,
  largest,
  loadOverview,
  phaseTree,
  share,
  topSpans,
} from "@/lib/overview";
import type { Overview, PhaseNode } from "@/lib/overview";
import type { CostRow, GroupRow, HotRow, TickRow } from "@/lib/types";
import { spanId } from "@/lib/info";
import { RowTools } from "./row-tools";
import { Segmented } from "./segmented";
import { Spark } from "./spark";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

type HotBy = "self" | "total";

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function Tile({
  title,
  value,
  unit,
  rows,
  children,
}: {
  title: string;
  value: string;
  unit?: string;
  rows: [string, string][];
  children?: ReactNode;
}) {
  return (
    <section className={styles.stat}>
      <h3 className={styles.statTitle}>{title}</h3>
      <p className={styles.statValue}>
        {value}
        {unit ? <span className={styles.statUnit}>{unit}</span> : null}
      </p>
      {children}
      <dl className={styles.statRows}>
        {rows.map(([label, text]) => (
          <div key={label} className={styles.statRow}>
            <dt>{label}</dt>
            <dd>{text}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

function Card({
  title,
  note,
  actions,
  children,
}: {
  title: string;
  note?: string;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className={styles.card}>
      <header className={styles.cardHead}>
        <div className={styles.cardHeading}>
          <h3 className={styles.cardTitle}>{title}</h3>
          {note ? <p className={styles.cardNote}>{note}</p> : null}
        </div>
        {actions ? <div className={styles.cardActions}>{actions}</div> : null}
      </header>
      {children}
    </section>
  );
}

/** A row of a meter list: a name, a bar, a value and a second value. */
function Meter({
  id,
  name,
  title,
  indent = 0,
  width,
  tone = "accent",
  value,
  extra,
  extraColor,
  strong = false,
}: {
  id?: string;
  name: string;
  title?: string;
  indent?: number;
  width: number;
  tone?: "accent" | "good" | "warn";
  value: string;
  extra?: string;
  extraColor?: string;
  strong?: boolean;
}) {
  return (
    <div className={strong ? styles.meterRowStrong : styles.meterRow} title={title}>
      <span className={styles.meterName} style={indent ? { paddingLeft: indent } : undefined}>
        <span className={styles.nameText}>{name}</span>
        <RowTools id={id} name={name} />
      </span>
      <span className={styles.meterTrack} aria-hidden="true">
        <span
          className={styles.meterFill}
          data-tone={tone}
          style={{ width: `${width}%`, minWidth: width > 0 ? 2 : 0 }}
        />
      </span>
      <span className={styles.meterValue}>{value}</span>
      <span className={styles.meterExtra} style={extraColor ? { color: extraColor } : undefined}>
        {extra}
      </span>
    </div>
  );
}

function MeterHead({ name, value, extra }: { name: string; value: string; extra?: string }) {
  return (
    <div className={styles.meterHead} aria-hidden="true">
      <span>{name}</span>
      <span />
      <span>{value}</span>
      <span>{extra}</span>
    </div>
  );
}

function Empty({ text }: { text: string }) {
  return <p className={styles.empty}>{text}</p>;
}

/** A phase of the tick, drawn as a row of the span tree `query spans` describes. */
function Phase({ node, depth, max }: { node: PhaseNode; depth: number; max: number }) {
  const pct = node.pct;
  const extraColor =
    pct >= 30 ? "var(--ink)" : pct >= 10 ? "var(--ink-soft)" : "var(--muted)";
  return (
    <Meter
      id={node.id}
      name={node.name}
      title={node.name}
      indent={Math.min(depth * 8, 32)}
      width={share(node.ms, max)}
      value={num(node.ms, 2)}
      extra={`${num(node.pct, 1)}%`}
      extraColor={extraColor}
      strong={depth === 0}
    />
  );
}

export interface ViewProps {
  capture: LoadedCapture;
  data: Overview;
  /** `horizon-perf summary`'s text, for the copy button. */
  text: string;
}

/** The summary's drawing (§7.2, UI1): tiles, the tick's span tree and the lists under it. */
export function SummaryView({ capture, data, text }: ViewProps) {
  const [hotBy, setHotBy] = useState<HotBy>("self");
  const [copied, setCopied] = useState(false);

  const fields = useMemo(() => byField(data.series), [data]);
  const phases = useMemo(() => flatPhases(phaseTree(data.spans)), [data]);
  const phaseMax = useMemo(() => largest(phases.map(([node]) => node.ms)), [phases]);

  const get = (name: string): Stats | null => stats(fields.get(name)?.values ?? []);
  const side = capture.side;
  const meta = capture.meta.rows.find((row) => row.side === side);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  };

  const tps = get("h:tps");
  const mspt = get("h:mspt_avg");
  const worstSecond = get("h:mspt_p95");
  const peak = get("h:mspt_max");
  const fps = get("h:fps");
  const frame = get("h:frame_ms_p95");
  const frameP50 = get("h:frame_ms_p50");
  const frameP99 = get("h:frame_ms_p99");
  const frameMax = get("h:frame_ms_max");
  const cpu = get("h:cpu_process_cores");
  const system = get("h:cpu_system_cores");
  const rss = get("h:rss_mb");
  const physical = get("h:physical_used_mb");
  const swap = get("h:swap_used_mb");
  const players = get("h:players");
  const chunks = get("h:chunks");
  const entities = get("h:entities");

  const resident = fields.get("h:rss_mb")?.values ?? [];
  const firstResident = resident.find((value): value is number => typeof value === "number");
  const change =
    firstResident !== undefined && rss?.last !== undefined ? rss.last - firstResident : null;

  const header = [
    t(`side.${side}`),
    meta ? `${num(meta.seconds, 1)} s` : null,
    meta?.build ? `${t("summary.build")} ${meta.build.version} ${meta.build.profile}` : null,
    meta?.machine ? `${meta.machine.os} ${meta.machine.arch}` : null,
    meta?.machine ? t("summary.threads", { count: meta.machine.cpu_threads }) : null,
  ].filter((part): part is string => Boolean(part));


  const hotMax = largest(data.hot.map((row) => (hotBy === "self" ? row.self : row.total)));
  const entityRows = attributed(data.entities);
  const blockRows = attributed(data.blocks);
  const entityMax = largest(entityRows.map((row) => row.value));
  const blockMax = largest(blockRows.map((row) => row.value));
  const costMax = largest(data.costs.map((row) => row.ms_per_tick));

  const hasSide = entityRows.length > 0 || blockRows.length > 0 || data.costs.length > 0;
  const frameCpu = get("h:frame_cpu_ms_p50");
  const frameGpu = get("h:frame_gpu_ms_p50");
  const allocs = get("h:allocs_per_s");
  const allocRate = get("h:alloc_mib_per_s");
  const commit = get("h:allocator_commit_mb");

  return (
    <div className={styles.summary}>
      <header className={styles.summaryHead}>
        <div>
          <p className={styles.summaryMeta}>{header.join(" · ")}</p>
        </div>
        {text ? (
          <button type="button" className={styles.buttonSmall} title={t("summary.copyHint")} onClick={() => void copy()}>
            {copied ? t("summary.copied") : t("summary.copy")}
          </button>
        ) : null}
      </header>

      <div className={styles.cards}>
        {side === "server" ? (
          <Tile
            title={t("health.tps")}
            value={num(tps?.last ?? null, 1)}
            unit={t("health.unitTps")}
            rows={[
              [t("health.window1m"), num(data.trailing[0] ?? null, 1)],
              [t("health.window5m"), num(data.trailing[1] ?? null, 1)],
              [t("health.window15m"), num(data.trailing[2] ?? null, 1)],
            ]}
          >
            <Spark values={fields.get("h:tps")?.values ?? []} label={t("health.tps")} />
          </Tile>
        ) : (
          <Tile
            title={t("health.fps")}
            value={num(fps?.last ?? null, 1)}
            unit={t("health.unitFps")}
            rows={[
              [t("health.min"), num(fps?.min ?? null, 1)],
              [t("health.median"), num(fps?.median ?? null, 1)],
              [t("health.max"), num(fps?.max ?? null, 1)],
            ]}
          >
            <Spark values={fields.get("h:fps")?.values ?? []} label={t("health.fps")} />
          </Tile>
        )}

        {side === "server" ? (
          <Tile
            title={t("health.mspt")}
            value={num(mspt?.mean ?? null, 1)}
            unit={t("health.unitMs")}
            rows={[
              [t("health.min"), num(mspt?.min ?? null, 1)],
              [t("health.median"), num(mspt?.median ?? null, 1)],
              [t("health.worstP95"), num(worstSecond?.max ?? null, 1)],
              [t("health.max"), num(peak?.max ?? null, 1)],
            ]}
          >
            <Spark values={fields.get("h:mspt_p95")?.values ?? []} label={t("health.mspt")} />
          </Tile>
        ) : (
          <Tile
            title={t("health.frame")}
            value={num(frame?.median ?? null, 1)}
            unit={t("health.unitMs")}
            rows={[
              [t("health.p50"), num(frameP50?.median ?? null, 1)],
              [t("health.p99"), num(frameP99?.median ?? null, 1)],
              [t("health.max"), num(frameMax?.max ?? null, 1)],
              ...(frameCpu ? ([[t("health.cpu"), num(frameCpu.median, 1)]] as [string, string][]) : []),
              ...(frameGpu ? ([[t("health.gpu"), num(frameGpu.median, 1)]] as [string, string][]) : []),
            ]}
          >
            <Spark values={fields.get("h:frame_ms_p95")?.values ?? []} label={t("health.frame")} />
          </Tile>
        )}

        <Tile
          title={t("health.cpuTitle")}
          value={num(cpu?.mean ?? null, 2)}
          unit={t("health.unitCores")}
          rows={[
            [
              t("health.system"),
              meta?.machine
                ? `${num(system?.mean ?? null, 2)} / ${meta.machine.cpu_threads}`
                : num(system?.mean ?? null, 2),
            ],
          ]}
        >
          <Spark
            values={fields.get("h:cpu_process_cores")?.values ?? []}
            label={t("health.cpuTitle")}
          />
        </Tile>

        <Tile
          title={t("health.memory")}
          value={mib(rss?.last)}
          rows={[
            [t("health.physical"), mib(physical?.last)],
            [t("health.swap"), mib(swap?.last)],
            [t("summary.change"), change === null ? "—" : `${num(change, 0)} MB`],
          ]}
        >
          <Spark values={resident} label={t("health.memory")} />
        </Tile>

        {allocs || allocRate ? (
          <Tile
            title={t("health.allocations")}
            value={withUnit(allocs?.mean, t("health.unitPerS"), 0)}
            rows={[
              [t("health.allocated"), withUnit(allocRate?.mean, t("health.unitMibPerS"), 2)],
              [t("health.commit"), mib(commit?.last)],
            ]}
          />
        ) : null}

        {side === "server" ? (
          <Tile
            title={t("health.world")}
            value={num(players?.last ?? null, 0)}
            unit={players?.last === 1 ? t("health.player") : t("health.players")}
            rows={[
              [t("health.chunks"), num(chunks?.last ?? null, 0)],
              [t("health.entities"), num(entities?.last ?? null, 0)],
            ]}
          />
        ) : chunks ? (
          <Tile
            title={t("health.world")}
            value={num(chunks.last, 0)}
            unit={t("health.chunks")}
            rows={[[t("health.entities"), num(entities?.last ?? null, 0)]]}
          />
        ) : null}
      </div>

      {phases.length > 0 ? (
      <Card
        title={t("summary.phases")}
        note={t("summary.phasesNote")}
        actions={
          data.ticks === null ? undefined : (
            <span className={styles.cardCount}>
              {t(side === "client" ? "summary.frames" : "summary.ticks", {
                count: grouped(data.ticks),
              })}
            </span>
          )
        }
      >
        <div className={styles.meterList}>
          <MeterHead
            name={t("summary.phase")}
            value={t("summary.msPerTick")}
            extra={t("summary.pctOfTick")}
          />
          {phases.map(([node, depth]) => (
            <Phase key={node.id} node={node} depth={depth} max={phaseMax} />
          ))}
        </div>
      </Card>
      ) : null}

      <div className={hasSide ? styles.split : styles.stack}>
        <div className={styles.stack}>
          {data.hot.length > 0 ? (
            <Card
              title={t("summary.hot")}
              note={t("summary.hotNote", { samples: grouped(data.samples ?? 0) })}
              actions={
                <Segmented
                  label={t("summary.hot")}
                  options={(["self", "total"] as const).map((id) => ({ id, text: t(`summary.${id}`) }))}
                  value={hotBy}
                  onChange={setHotBy}
                />
              }
            >
              <div className={styles.meterList}>
                <MeterHead
                  name={t("summary.function")}
                  value={t("summary.self")}
                  extra={t("summary.total")}
                />
                {data.hot.map((row: HotRow) => (
                  <Meter
                    key={row.id}
                    id={row.id}
                    name={row.name}
                    title={row.full}
                    width={share(hotBy === "self" ? row.self : row.total, hotMax)}
                    tone="good"
                    value={num(row.self, 2)}
                    extra={num(row.total, 2)}
                  />
                ))}
              </div>
            </Card>
          ) : null}

          {data.worst.length > 0 ? (
            <Card
              title={t(side === "client" ? "summary.worstFrames" : "summary.worst")}
              note={t(side === "client" ? "summary.worstFramesNote" : "summary.worstNote")}
            >
              <div className={styles.ticks}>
                {data.worst.map((tick: TickRow) => (
                  <div key={tick.id} className={styles.tickRow}>
                    <details className={styles.tick}>
                      <summary className={styles.tickHead}>
                        <span className={styles.tickMs}>{num(tick.wall_ms, 1)} ms</span>
                        <span className={styles.tickAt}>
                          {t("summary.at", { time: at(tick.t) })}
                        </span>
                        <span className={styles.tickSpans}>
                          {topSpans(tick).map(([name, , ms]) => (
                            <span key={name} className={styles.chip}>
                              {name} {num(ms, 1)}
                            </span>
                          ))}
                        </span>
                      </summary>
                      <div className={styles.tickBody}>
                        {tick.spans.map(([name, depth, wall], index) => (
                          <div
                            key={`${name}-${index}`}
                            className={styles.tickSpan}
                            style={{ paddingLeft: depth * 12 }}
                          >
                            <span className={styles.tickSpanName}>
                              <span className={styles.nameText}>{name}</span>
                              <RowTools id={spanId(name)} name={name} />
                            </span>
                            <span className={styles.tickSpanMs}>{num(wall, 2)} ms</span>
                          </div>
                        ))}
                      </div>
                    </details>
                    <span className={styles.tickTools}>
                      <RowTools id={tick.id} name={at(tick.t)} />
                    </span>
                  </div>
                ))}
              </div>
            </Card>
          ) : null}
        </div>

        <div className={styles.stack}>
          {entityRows.length > 0 ? (
            <Card title={t("summary.entities")} note={t("summary.entitiesNote")}>
              <div className={styles.meterList}>
                {entityRows.map((row: GroupRow) => (
                  <Meter
                    key={row.key}
                    id={row.key ?? undefined}
                    name={shortId(row.label)}
                    title={row.key ?? undefined}
                    width={share(row.value, entityMax)}
                    value={num(row.value, 2)}
                    extra={`${num(row.pct, 1)}%`}
                  />
                ))}
              </div>
            </Card>
          ) : null}

          {blockRows.length > 0 ? (
            <Card title={t("summary.blocks")} note={t("summary.blocksNote")}>
              <div className={styles.meterList}>
                {blockRows.map((row: GroupRow) => (
                  <Meter
                    key={row.key}
                    id={row.key ?? undefined}
                    name={shortId(row.label)}
                    title={row.key ?? undefined}
                    width={share(row.value, blockMax)}
                    value={num(row.value, 2)}
                    extra={`${num(row.pct, 1)}%`}
                  />
                ))}
              </div>
            </Card>
          ) : null}

          {data.costs.length > 0 ? (
            <Card title={t("summary.mobs")} note={t("summary.mobsNote")}>
              <div className={styles.meterList}>
                <MeterHead
                  name={t("summary.type")}
                  value={t("summary.msPerTick")}
                  extra={t("summary.usPerMob")}
                />
                {data.costs.map((row: CostRow) => (
                  <Meter
                    key={row.id}
                    id={row.id}
                    name={shortId(row.name)}
                    title={row.name}
                    width={share(row.ms_per_tick, costMax)}
                    tone="warn"
                    value={num(row.ms_per_tick, 2)}
                    extra={num(row.us_per_mob, 1)}
                  />
                ))}
              </div>
            </Card>
          ) : null}
        </div>
      </div>
    </div>
  );
}

/** The Summary tab: loads what the view draws, then draws it. */
export function SummaryPanel({ reader, capture }: Props) {
  const [text, setText] = useState("");
  const [data, setData] = useState<Overview | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    setData(null);
    try {
      setData(loadOverview(reader, capture.handle, capture.name, capture.side, capture.meta));
    } catch (failure) {
      setError(message(failure));
    }
    // A capture with no report only loses the copy button.
    try {
      const report = reader.summary(capture.handle, { file: capture.name, side: capture.side });
      const lead = `${capture.name}: `;
      setText(report.startsWith(lead) ? report.slice(lead.length) : report);
    } catch {
      setText("");
    }
  }, [reader, capture]);

  if (error) return <p className={styles.error}>{error}</p>;
  if (!data) return null;
  return <SummaryView capture={capture} data={data} text={text} />;
}
