"use client";

import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import type { HorizonPerf } from "@/lib/generated/horizon_perf";
import { bool, duration, grouped, mib, num, utc } from "@/lib/format";
import { t } from "@/lib/i18n";
import type {
  DimensionRow,
  Envelope,
  MemoryRow,
  MetaRow,
  NetPlayerRow,
  NetTotalRow,
} from "@/lib/types";
import type { LoadedCapture } from "./use-capture";
import styles from "./page.module.css";

interface Props {
  reader: HorizonPerf;
  capture: LoadedCapture;
}

type Tab = "overview" | "host" | "memory" | "network" | "flags" | "config" | "world" | "rules" | "mods";

const TABS: [Tab, string][] = [
  ["overview", "meta.tabOverview"],
  ["host", "meta.tabHost"],
  ["memory", "meta.tabMemory"],
  ["network", "meta.tabNetwork"],
  ["flags", "meta.tabFlags"],
  ["config", "meta.tabConfig"],
  ["world", "meta.tabWorld"],
  ["rules", "meta.tabRules"],
  ["mods", "meta.tabMods"],
];

function show(value: string | number | boolean | null | undefined): string {
  if (value === null || value === undefined || value === "") return t("meta.notDetected");
  if (typeof value === "boolean") return bool(value, t("common.yes"), t("common.no"));
  return String(value);
}

function KV({ items }: { items: [string, ReactNode][] }) {
  const shown = items.filter(([, value]) => value !== null && value !== undefined && value !== "");
  if (shown.length === 0) return <p className={styles.empty}>{t("meta.empty")}</p>;
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
  if (values.length === 0) return <p className={styles.empty}>{t("meta.none")}</p>;
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

/** The metadata panel (UI1): everything the capture's `Start` says, one tab per subject. */
export function MetadataPanel({ reader, capture }: Props) {
  const [picked, setPicked] = useState<Tab>("overview");
  const [memory, setMemory] = useState<MemoryRow[]>([]);
  const [net, setNet] = useState<Envelope<NetTotalRow | NetPlayerRow> | null>(null);
  const [world, setWorld] = useState<DimensionRow[]>([]);

  useEffect(() => {
    const request = <R,>(op: string) =>
      reader.query<R>(capture.handle, { op, file: capture.name, filters: { side: capture.side } });
    const memoryReply = request<MemoryRow>("memory");
    setMemory(memoryReply.ok ? memoryReply.rows : []);
    setNet(request<NetTotalRow | NetPlayerRow>("net"));
    const worldReply = request<DimensionRow>("world");
    setWorld(worldReply.ok ? worldReply.rows : []);
  }, [reader, capture]);

  const row: MetaRow | undefined = capture.meta.rows.find((entry) => entry.side === capture.side);
  const build = row?.build;
  const machine = row?.machine;
  const options = row?.options;
  const settings = row?.settings;
  const rules = row?.game_rules;
  const horizon = row?.horizon_options;

  const netRows = net?.rows ?? [];
  const total = netRows.find((entry): entry is NetTotalRow => "players" in entry);
  const players = netRows.filter((entry): entry is NetPlayerRow => "index" in entry);
  const empty: Partial<Record<Tab, boolean>> = {
    memory: memory.length === 0,
    network: !total || total.out_kib_s + total.in_kib_s === 0,
    world: world.length === 0,
    mods: !row?.mods.length,
  };
  const shown = TABS.filter(([id]) => !empty[id]);
  const tab = shown.some(([id]) => id === picked) ? picked : "overview";

  return (
    <div className={styles.metadata}>
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

      {tab === "overview" ? (
        <div className={styles.columns}>
          <section>
            <h3>{t("meta.capture")}</h3>
            <KV
              items={[
                [t("meta.kind"), show(row?.kind)],
                [t("meta.trigger"), show(row?.trigger)],
                [t("meta.started"), utc(row?.started_unix_ms)],
                [t("meta.duration"), duration(row?.seconds)],
                [t("meta.uptime"), duration(row?.uptime_s)],
                [t("meta.players"), show(row?.players)],
                [t("meta.sameProcess"), show(row?.same_process)],
                [
                  t("meta.worldFiles"),
                  row
                    ? `${grouped(row.world_fs_used_mb)} / ${grouped(row.world_fs_total_mb)} MiB`
                    : "—",
                ],
                [t("meta.stop"), show(row?.stop?.reason)],
              ]}
            />
          </section>
          <section>
            <h3>{t("meta.build")}</h3>
            <KV
              items={[
                [t("meta.version"), show(build?.version)],
                [t("meta.commit"), show(build?.commit)],
                [t("meta.profile"), show(build?.profile)],
                [t("meta.target"), show(build?.target)],
                [t("meta.symbols"), show(build?.symbols)],
              ]}
            />
            <h3>{t("meta.features")}</h3>
            <Chips values={build?.features ?? []} />
          </section>
        </div>
      ) : null}

      {tab === "host" ? (
        <section>
          <h3>{t("meta.machine")}</h3>
          <KV
            items={[
              [t("meta.os"), show(machine?.os)],
              [t("meta.osVersion"), show(machine?.os_version)],
              [t("meta.arch"), show(machine?.arch)],
              [t("meta.cpu"), show(machine?.cpu)],
              [t("meta.threads"), show(machine?.cpu_threads)],
              [t("meta.cores"), show(machine?.cpu_cores)],
              [t("meta.ram"), machine ? mib(machine.ram_mb) : "—"],
              [t("meta.cgroupCpu"), machine?.cgroup_cpu_cores == null ? "Not detected" : num(machine.cgroup_cpu_cores, 2)],
              [t("meta.cgroupMemory"), machine?.cgroup_memory_mb ? mib(machine.cgroup_memory_mb) : "Not detected"],
              [t("meta.gpu"), show(machine?.gpu)],
            ]}
          />
        </section>
      ) : null}

      {tab === "memory" ? (
        memory.length === 0 ? (
          <p className={styles.empty}>{t("meta.noMemory")}</p>
        ) : (
          <table className={styles.table}>
            <thead>
              <tr>
                <th>{t("meta.path")}</th>
                <th>{t("meta.value")}</th>
              </tr>
            </thead>
            <tbody>
              {memory.map((entry) => (
                <tr key={entry.id}>
                  <td>{entry.path}</td>
                  <td>
                    {entry.unit === "mb" ? `${num(entry.value, 1)} MiB` : grouped(entry.value)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )
      ) : null}

      {tab === "network" ? (
        !net?.ok ? (
          <p className={styles.empty}>{t("meta.noNetwork")}</p>
        ) : (
          <>
            <KV
              items={[
                [t("meta.players"), show(total?.players)],
                [t("meta.out"), total ? `${num(total.out_kib_s)} KiB/s` : "—"],
                [t("meta.in"), total ? `${num(total.in_kib_s)} KiB/s` : "—"],
              ]}
            />
            <table className={styles.table}>
              <thead>
                <tr>
                  <th>{t("meta.player")}</th>
                  <th>{t("meta.out")}</th>
                  <th>{t("meta.in")}</th>
                  <th>{t("meta.packetsOut")}</th>
                  <th>{t("meta.packetsIn")}</th>
                </tr>
              </thead>
              <tbody>
                {players.map((player) => (
                  <tr key={player.id}>
                    <td>{player.index}</td>
                    <td>{num(player.out_kib_s)} KiB/s</td>
                    <td>{num(player.in_kib_s)} KiB/s</td>
                    <td>{grouped(player.packets_out)}</td>
                    <td>{grouped(player.packets_in)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </>
        )
      ) : null}

      {tab === "flags" ? (
        <div className={styles.columns}>
          <section>
            <h3>{t("meta.profiler")}</h3>
            <KV
              items={[
                [t("meta.hz"), show(options?.hz)],
                [t("meta.threadsOption"), show(options?.threads)],
                [t("meta.ticksOver"), num(options?.ticks_over_ms ?? null, 1)],
                [t("meta.wall"), show(options?.wall)],
                [t("meta.timeline"), show(options?.timeline)],
                [t("meta.gpuPasses"), show(options?.gpu)],
                [t("meta.samples"), show(options?.samples)],
                [t("meta.window"), num(options?.window_s ?? null, 1)],
              ]}
            />
          </section>
          <section>
            <h3>{t("meta.horizonSwitches")}</h3>
            <KV
              items={[
                [t("meta.maxViewDistance"), show(horizon?.max_view_distance)],
                [t("meta.lodDistance"), show(horizon?.lod_distance)],
                [t("meta.anticheat"), show(horizon?.anticheat)],
              ]}
            />
          </section>
          <section>
            <h3>{t("meta.features")}</h3>
            <Chips values={build?.features ?? []} />
          </section>
        </div>
      ) : null}

      {tab === "config" ? (
        <KV
          items={[
            [t("meta.viewDistance"), show(settings?.view_distance)],
            [t("meta.simulationDistance"), show(settings?.simulation_distance)],
            [t("meta.maxPlayers"), show(settings?.max_players)],
            [t("meta.compression"), show(settings?.network_compression_threshold)],
          ]}
        />
      ) : null}

      {tab === "world" ? (
        world.length === 0 ? (
          <p className={styles.empty}>{t("meta.noWorld")}</p>
        ) : (
          <table className={styles.table}>
            <thead>
              <tr>
                <th>{t("meta.dimension")}</th>
                <th>{t("meta.chunks")}</th>
                <th>{t("meta.entities")}</th>
                <th>{t("meta.blockEntities")}</th>
              </tr>
            </thead>
            <tbody>
              {world.map((dimension) => (
                <tr key={dimension.id}>
                  <td>{dimension.name}</td>
                  <td>{grouped(dimension.chunks)}</td>
                  <td>{grouped(dimension.entities)}</td>
                  <td>{grouped(dimension.block_entities)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )
      ) : null}

      {tab === "rules" ? (
        <KV
          items={[
            [t("meta.randomTickSpeed"), show(rules?.random_tick_speed)],
            [t("meta.maxEntityCramming"), show(rules?.max_entity_cramming)],
            [t("meta.fireSpread"), show(rules?.fire_spread_radius_around_player)],
            [t("meta.advanceTime"), show(rules?.advance_time)],
            [t("meta.mobGriefing"), show(rules?.mob_griefing)],
            [t("meta.spawnMobs"), show(rules?.spawn_mobs)],
            [t("meta.spawnMonsters"), show(rules?.spawn_monsters)],
            [t("meta.spawnerBlocks"), show(rules?.spawner_blocks_work)],
          ]}
        />
      ) : null}

      {tab === "mods" ? (
        (row?.mods.length ?? 0) === 0 ? (
          <p className={styles.empty}>{t("meta.noMods")}</p>
        ) : (
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
                  <td>{mod.kind}</td>
                  <td>{mod.crate_name || "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )
      ) : null}
    </div>
  );
}
