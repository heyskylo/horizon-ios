"use client";

import { useState } from "react";
import type { ReactNode } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { bool, duration, grouped, mib, num, utc } from "@/lib/format";
import { t } from "@/lib/i18n";
import type { MetaRow } from "@/lib/types";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

type Tab = "overview" | "host" | "options" | "settings" | "rules" | "mods";

type Item = [string, ReactNode];

/** A value the capture recorded, or null so its row is left out. */
function seen(value: string | number | boolean | null | undefined, text?: (value: string | number) => string): string | null {
  if (value === null || value === undefined || value === "") return null;
  if (typeof value === "boolean") return bool(value, t("common.yes"), t("common.no"));
  return text ? text(value) : String(value);
}

/** A name from the closed set the wasm answers with; "unspecified" and "unknown" are nothing to show. */
function named(group: string, value: string | null | undefined): string | null {
  return !value || value === "unspecified" || value === "unknown" ? null : t(`${group}.${value}`);
}

function KV({ items }: { items: Item[] }) {
  const shown = items.filter(([, value]) => value !== null && value !== undefined && value !== "");
  if (shown.length === 0) return null;
  return (
    <dl className={styles.kv}>
      {shown.map(([key, value]) => (
        <div key={key} className={styles.kvRow}>
          <dt>{key}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

function Chips({ values }: { values: string[] }) {
  return (
    <ul className={styles.chips}>
      {values.map((value) => (
        <li key={value} className={styles.chip}>
          {value}
        </li>
      ))}
    </ul>
  );
}

function present(items: Item[]): boolean {
  return items.some(([, value]) => value !== null && value !== undefined && value !== "");
}

function sections(row: MetaRow | undefined, side: MetaRow["side"]) {
  const build = row?.build;
  const machine = row?.machine;
  const options = row?.options;
  const settings = row?.settings;
  const rules = row?.game_rules;
  const horizon = row?.horizon_options;

  const capture: Item[] = [
    [t("meta.kind"), named("meta.kind", row?.kind)],
    [t("meta.trigger"), named("meta.trigger", row?.trigger)],
    [t("meta.started"), row?.started_unix_ms ? utc(row.started_unix_ms) : null],
    [t("meta.duration"), row ? duration(row.seconds) : null],
    [t("meta.uptime"), row && row.uptime_s >= 1 ? duration(row.uptime_s) : null],
    [t("meta.players"), side === "server" ? seen(row?.players) : null],
    [
      t("meta.worldFiles"),
      row && row.world_fs_total_mb > 0
        ? t("meta.worldFilesValue", { used: grouped(row.world_fs_used_mb), total: grouped(row.world_fs_total_mb) })
        : null,
    ],
    [t("meta.stop"), named("meta.stop", row?.stop?.reason)],
  ];
  const buildItems: Item[] = [
    [t("meta.version"), seen(build?.version)],
    [t("meta.commit"), seen(build?.commit)],
    [t("meta.profile"), seen(build?.profile)],
    [t("meta.target"), seen(build?.target)],
    [t("meta.symbols"), named("meta.symbols", build?.symbols)],
  ];
  const host: Item[] = [
    [t("meta.os"), seen(machine?.os)],
    [t("meta.osVersion"), seen(machine?.os_version)],
    [t("meta.arch"), seen(machine?.arch)],
    [t("meta.cpu"), seen(machine?.cpu)],
    [t("meta.threads"), seen(machine?.cpu_threads)],
    [t("meta.cores"), seen(machine?.cpu_cores)],
    [t("meta.ram"), machine?.ram_mb ? mib(machine.ram_mb) : null],
    [t("meta.cgroupCpu"), machine?.cgroup_cpu_cores == null ? null : num(machine.cgroup_cpu_cores, 2)],
    [t("meta.cgroupMemory"), machine?.cgroup_memory_mb ? mib(machine.cgroup_memory_mb) : null],
    [t("meta.gpu"), seen(machine?.gpu)],
  ];
  const profiler: Item[] = [
    [t("meta.hz"), options?.hz ? t("meta.perSecond", { count: options.hz }) : null],
    [t("meta.threadsOption"), named("meta.threadsValue", options?.threads)],
    [t("meta.ticksOver"), options?.ticks_over_ms == null ? null : `${num(options.ticks_over_ms, 1)} ms`],
    [t("meta.wall"), seen(options?.wall)],
    [t("meta.timeline"), seen(options?.timeline)],
    [t("meta.gpuPasses"), named("meta.gpuValue", options?.gpu)],
    [t("meta.samples"), seen(options?.samples)],
    [t("meta.window"), options?.window_s == null ? null : `${num(options.window_s, 0)} s`],
  ];
  const switches: Item[] = [
    [t("meta.maxViewDistance"), seen(horizon?.max_view_distance)],
    [t("meta.lodDistance"), seen(horizon?.lod_distance)],
    [t("meta.anticheat"), seen(horizon?.anticheat)],
  ];
  const config: Item[] = [
    [t("meta.viewDistance"), seen(settings?.view_distance)],
    [t("meta.simulationDistance"), seen(settings?.simulation_distance)],
    [t("meta.maxPlayers"), seen(settings?.max_players)],
    [
      t("meta.compression"),
      settings?.network_compression_threshold == null
        ? null
        : settings.network_compression_threshold < 0
          ? t("meta.compressionOff")
          : t("meta.compressionFrom", { bytes: settings.network_compression_threshold }),
    ],
  ];
  const gameRules: Item[] = [
    [t("meta.randomTickSpeed"), seen(rules?.random_tick_speed)],
    [t("meta.maxEntityCramming"), seen(rules?.max_entity_cramming)],
    [t("meta.fireSpread"), seen(rules?.fire_spread_radius_around_player)],
    [t("meta.advanceTime"), seen(rules?.advance_time)],
    [t("meta.mobGriefing"), seen(rules?.mob_griefing)],
    [t("meta.spawnMobs"), seen(rules?.spawn_mobs)],
    [t("meta.spawnMonsters"), seen(rules?.spawn_monsters)],
    [t("meta.spawnerBlocks"), seen(rules?.spawner_blocks_work)],
  ];
  return { capture, buildItems, host, profiler, switches, config, gameRules };
}

/** The metadata panel: what the capture says about itself, one tab per subject that has something to say. */
export function MetadataPanel({ capture }: Props) {
  const [picked, setPicked] = useState<Tab>("overview");

  const row: MetaRow | undefined = capture.meta.rows.find((entry) => entry.side === capture.side);
  const { capture: about, buildItems, host, profiler, switches, config, gameRules } = sections(row, capture.side);
  const features = row?.build?.features ?? [];

  const tabs: [Tab, string, boolean][] = [
    ["overview", "meta.tabOverview", true],
    ["host", "meta.tabHost", present(host)],
    ["options", "meta.tabOptions", present(profiler) || present(switches)],
    ["settings", "meta.tabSettings", present(config)],
    ["rules", "meta.tabRules", present(gameRules)],
    ["mods", "meta.tabMods", (row?.mods.length ?? 0) > 0],
  ];
  const shown = tabs.filter(([, , has]) => has);
  const tab = shown.some(([id]) => id === picked) ? picked : "overview";

  return (
    <div className={styles.metadata}>
      {shown.length > 1 ? (
        <nav className={styles.tabBar} aria-label={t("meta.tabs")}>
          {shown.map(([id, label]) => (
            <button
              key={id}
              type="button"
              className={tab === id ? styles.tabActive : styles.tab}
              onClick={() => setPicked(id)}
            >
              {t(label)}
            </button>
          ))}
        </nav>
      ) : null}

      {tab === "overview" ? (
        <div className={styles.columns}>
          <section>
            <h3>{t("meta.capture")}</h3>
            <KV items={about} />
          </section>
          <section>
            <h3>{t("meta.build")}</h3>
            <KV items={buildItems} />
            {features.length > 0 ? (
              <>
                <h3>{t("meta.features")}</h3>
                <Chips values={features} />
              </>
            ) : null}
          </section>
        </div>
      ) : null}

      {tab === "host" ? (
        <section>
          <h3>{t("meta.machine")}</h3>
          <KV items={host} />
        </section>
      ) : null}

      {tab === "options" ? (
        <div className={styles.columns}>
          {present(profiler) ? (
            <section>
              <h3>{t("meta.profiler")}</h3>
              <KV items={profiler} />
            </section>
          ) : null}
          {present(switches) ? (
            <section>
              <h3>{t("meta.horizonSwitches")}</h3>
              <KV items={switches} />
            </section>
          ) : null}
        </div>
      ) : null}

      {tab === "settings" ? <KV items={config} /> : null}

      {tab === "rules" ? <KV items={gameRules} /> : null}

      {tab === "mods" ? (
        <table className={styles.table}>
          <thead>
            <tr>
              <th>{t("meta.mod")}</th>
              <th>{t("meta.version")}</th>
              <th>{t("meta.modKind")}</th>
              <th>{t("meta.crate")}</th>
            </tr>
          </thead>
          <tbody>
            {(row?.mods ?? []).map((mod) => (
              <tr key={`${mod.id}:${mod.version}`}>
                <td>{mod.id}</td>
                <td>{mod.version}</td>
                <td>{named("meta.modKindValue", mod.kind) ?? "—"}</td>
                <td>{mod.crate || "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      ) : null}
    </div>
  );
}
