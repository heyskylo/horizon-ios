// Temporary snapshot for a visual check. Deleted after the check.
import type { LoadedCapture } from "@/app/perf/use-capture";
import type { Overview } from "@/lib/overview";

export const snapshot: { capture: LoadedCapture; data: Overview; text: string } = {
 "capture": {
  "handle": 1,
  "name": "profile-20261108-132243Z.hzperf",
  "bytes": 99036,
  "meta": {
   "ok": true,
   "op": "meta",
   "captures": [
    {
     "file": "profile-20261108-132243Z.hzperf",
     "sides": [
      "server",
      "client"
     ],
     "kind": "integrated",
     "seconds": 44.398
    }
   ],
   "selection": {
    "side": "server",
    "thread": "Server thread",
    "select": "cpu",
    "slow": false,
    "where": {},
    "from": 0,
    "to": 44.398,
    "ticks": 886,
    "samples": null
   },
   "unit": "meta",
   "rows": [
    {
     "side": "server",
     "seconds": 44.398,
     "sections": {
      "history": 0,
      "slow_history": 0,
      "world": 2,
      "marks": 0,
      "warnings": 0,
      "health": 43,
      "counters": 0,
      "slow": 0,
      "span_blocks": 9,
      "memory": 0,
      "spans": 1,
      "workers": 1,
      "net": 1,
      "gpu": 0,
      "samples": 1
     },
     "kind": "integrated",
     "trigger": "env",
     "started_unix_ms": 1794144164049,
     "uptime_s": 0,
     "same_process": true,
     "players": 0,
     "world_fs_used_mb": 0,
     "world_fs_total_mb": 0,
     "build": {
      "version": "0.1.0",
      "commit": "",
      "profile": "release",
      "target": "x86_64-linux",
      "features": [],
      "symbols": "symtab"
     },
     "machine": {
      "os": "linux",
      "os_version": "Fedora Linux 45 (KDE Plasma Desktop Edition) (kernel 7.2.15-300.fc45.x86_64)",
      "arch": "x86_64",
      "cpu": "Intel(R) Core(TM) Ultra 5 245KF",
      "cpu_threads": 14,
      "cpu_cores": 14,
      "ram_mb": 31791,
      "cgroup_cpu_cores": null,
      "cgroup_memory_mb": null,
      "gpu": ""
     },
     "options": {
      "hz": 250,
      "threads": "main",
      "ticks_over_ms": null,
      "wall": false,
      "timeline": false,
      "gpu": "unspecified",
      "samples": true,
      "window_s": null
     },
     "settings": {
      "view_distance": 12,
      "simulation_distance": 12,
      "max_players": 8,
      "network_compression_threshold": -1
     },
     "game_rules": {
      "random_tick_speed": 3,
      "max_entity_cramming": 24,
      "fire_spread_radius_around_player": 128,
      "advance_time": true,
      "mob_griefing": true,
      "spawn_mobs": true,
      "spawn_monsters": true,
      "spawner_blocks_work": true
     },
     "mods": [],
     "stop": {
      "t": 44.398,
      "reason": "shutdown",
      "overhead": {
       "spans_ms_per_tick": 0.009,
       "spans_share": 0.001516,
       "sampler_share_of_thread": 0.000067,
       "writer_core_share": 0.00103,
       "dropped_samples": 0,
       "dropped_messages": 0,
       "timeline_stopped_t": null
      }
     }
    },
    {
     "side": "client",
     "seconds": 44.733,
     "sections": {
      "history": 0,
      "slow_history": 0,
      "world": 2,
      "marks": 0,
      "warnings": 0,
      "health": 43,
      "counters": 0,
      "slow": 0,
      "span_blocks": 9,
      "memory": 0,
      "spans": 1,
      "workers": 1,
      "net": 1,
      "gpu": 0,
      "samples": 1
     },
     "kind": "client",
     "trigger": "env",
     "started_unix_ms": 1794144163966,
     "uptime_s": 0.058,
     "same_process": false,
     "players": 0,
     "world_fs_used_mb": 0,
     "world_fs_total_mb": 0,
     "build": {
      "version": "0.1.0",
      "commit": "",
      "profile": "release",
      "target": "x86_64-linux",
      "features": [],
      "symbols": "symtab"
     },
     "machine": {
      "os": "linux",
      "os_version": "Fedora Linux 45 (KDE Plasma Desktop Edition) (kernel 7.2.15-300.fc45.x86_64)",
      "arch": "x86_64",
      "cpu": "Intel(R) Core(TM) Ultra 5 245KF",
      "cpu_threads": 14,
      "cpu_cores": 14,
      "ram_mb": 31791,
      "cgroup_cpu_cores": null,
      "cgroup_memory_mb": null,
      "gpu": "AMD Radeon RX 9070 XT (RADV GFX1201)"
     },
     "options": {
      "hz": 250,
      "threads": "main",
      "ticks_over_ms": null,
      "wall": false,
      "timeline": false,
      "gpu": "off",
      "samples": true,
      "window_s": null
     },
     "settings": null,
     "mods": [],
     "stop": {
      "t": 44.733,
      "reason": "shutdown",
      "overhead": {
       "spans_ms_per_tick": 0,
       "spans_share": 0,
       "sampler_share_of_thread": 0.000118,
       "writer_core_share": 0.00103,
       "dropped_samples": 0,
       "dropped_messages": 0,
       "timeline_stopped_t": null
      }
     }
    }
   ],
   "warnings": []
  },
  "side": "server",
  "sides": [
   "server",
   "client"
  ]
 },
 "data": {
  "seconds": 44.398,
  "ticks": 886,
  "samples": 1242,
  "series": [
   {
    "field": "h:tps",
    "unit": "ticks/s",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     19.083,
     20.835,
     20.083,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20,
     20
    ]
   },
   {
    "field": "h:mspt_avg",
    "unit": "ms",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     3.684,
     3.831,
     3.92,
     4.883,
     6.321,
     6.976,
     4.984,
     5.452,
     4.798,
     4.563,
     4.997,
     4.814,
     3.412,
     4.886,
     6.301,
     5.706,
     5.301,
     4.906,
     5.087,
     5.476,
     5.747,
     7.083,
     6.684,
     5.106,
     5.428,
     5.615,
     5.025,
     5.087,
     4.668,
     4.196,
     3.296,
     3.287,
     3.315,
     3.775,
     5.1,
     5.447,
     8.362,
     8.622,
     9.7,
     9.964,
     9.31,
     9.228,
     10.507,
     10.622
    ]
   },
   {
    "field": "h:mspt_p50",
    "unit": "ms",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     3.7,
     3.861,
     3.855,
     4.521,
     6.192,
     7.069,
     4.182,
     4.92,
     4.601,
     4.201,
     4.977,
     4.508,
     3.153,
     4.793,
     3.991,
     4.731,
     5.428,
     4.242,
     4.496,
     4.901,
     5.16,
     6.857,
     6.956,
     4.755,
     5.411,
     5.31,
     4.568,
     4.89,
     4.437,
     4.017,
     3.308,
     3.346,
     3.25,
     3.616,
     4.748,
     5.223,
     7.924,
     8.515,
     8.831,
     9.044,
     9.168,
     9.095,
     8.976,
     8.966
    ]
   },
   {
    "field": "h:mspt_p95",
    "unit": "ms",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     4.123,
     4.284,
     5.141,
     6.309,
     9.089,
     9.685,
     8.243,
     8.79,
     6.167,
     6.208,
     6.206,
     6.253,
     4.072,
     7.141,
     6.643,
     8.227,
     6.556,
     7.441,
     6.997,
     7.924,
     8.052,
     9.229,
     9.128,
     6.754,
     6.315,
     7.397,
     6.965,
     6.743,
     6.292,
     5.177,
     3.607,
     3.633,
     3.753,
     5.202,
     6.889,
     7.054,
     12.184,
     11.753,
     15.003,
     14.815,
     10.764,
     10.772,
     19.014,
     19.753
    ]
   },
   {
    "field": "h:mspt_max",
    "unit": "ms",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     4.171,
     4.774,
     7.108,
     6.574,
     11.077,
     10.003,
     9.107,
     10.912,
     7.884,
     7.097,
     7.612,
     7.01,
     6.154,
     7.323,
     49.938,
     12.453,
     7.762,
     8.208,
     8.344,
     8.578,
     8.399,
     11.059,
     9.305,
     7.803,
     7.374,
     8.975,
     8.274,
     7.694,
     6.546,
     7.245,
     4.109,
     3.666,
     3.885,
     6.268,
     7.182,
     7.712,
     13.38,
     16.17,
     16.968,
     18.978,
     12.165,
     11.621,
     26.446,
     27.781
    ]
   },
   {
    "field": "h:cpu_process_cores",
    "unit": "cores",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     6.06,
     6.06,
     0.675,
     0.243,
     1.12,
     0.658,
     0.37,
     0.308,
     0.201,
     0.254,
     0.249,
     0.249,
     0.204,
     0.226,
     0.211,
     0.28,
     0.235,
     0.294,
     0.261,
     0.241,
     0.239,
     0.283,
     0.289,
     0.39,
     0.279,
     0.27,
     0.26,
     0.284,
     0.271,
     0.252,
     0.213,
     0.254,
     0.223,
     0.245,
     0.213,
     0.328,
     0.343,
     0.357,
     0.442,
     0.376,
     0.442,
     0.44,
     0.439,
     0.439
    ]
   },
   {
    "field": "h:cpu_system_cores",
    "unit": "cores",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     7.772,
     7.772,
     2.814,
     2.501,
     3.063,
     2.608,
     2.481,
     1.518,
     1.219,
     1.509,
     2.354,
     2.096,
     2.026,
     1.992,
     2.27,
     1.578,
     1.465,
     1.383,
     1.878,
     1.129,
     1.337,
     1.381,
     1.406,
     1.538,
     1.572,
     3.117,
     3.534,
     5.202,
     3.394,
     3.213,
     3.35,
     3.291,
     3.428,
     3.363,
     3.443,
     12.251,
     13.588,
     13.797,
     13.856,
     13.751,
     13.646,
     13.814,
     13.83,
     13.83
    ]
   },
   {
    "field": "h:rss_mb",
    "unit": "MB",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     970,
     970,
     970,
     970,
     970,
     970,
     962.661,
     958.331,
     958,
     960.752,
     962.835,
     963,
     963,
     963,
     963,
     959.331,
     959,
     950.744,
     950,
     956.422,
     957,
     957,
     957,
     957,
     933.149,
     931,
     931,
     936.504,
     937,
     937,
     937,
     937,
     937,
     938.835,
     939,
     940.835,
     942.835,
     950.339,
     944.578,
     934.826,
     915.653,
     916.752,
     917,
     917
    ]
   },
   {
    "field": "h:physical_used_mb",
    "unit": "MB",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     8609,
     8609,
     8669.546,
     8680.504,
     8683.752,
     8702.347,
     8756.289,
     8723.388,
     8680.554,
     8650.397,
     8645.248,
     8661.512,
     8651.074,
     8611.471,
     8623.595,
     8636.926,
     8628.826,
     8618.826,
     8571.215,
     8556.909,
     8527.562,
     8514.909,
     8490.149,
     8487.083,
     8466.818,
     8781.489,
     8854.033,
     8846.992,
     8847.835,
     8839.744,
     8844.504,
     8799.132,
     8794.083,
     8822.438,
     8629.603,
     9499.086,
     9852.373,
     9939.38,
     10569.721,
     10808.554,
     10792.892,
     10884.488,
     10893,
     10893
    ]
   },
   {
    "field": "h:swap_used_mb",
    "unit": "MB",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0,
     0
    ]
   },
   {
    "field": "h:allocs_per_s",
    "unit": "per s",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null
    ]
   },
   {
    "field": "h:alloc_mib_per_s",
    "unit": "MiB/s",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null
    ]
   },
   {
    "field": "h:allocator_commit_mb",
    "unit": "MB",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null,
     null
    ]
   },
   {
    "field": "h:players",
    "unit": "count",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1,
     1
    ]
   },
   {
    "field": "h:chunks",
    "unit": "count",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     882,
     882,
     882,
     882,
     882,
     882,
     882,
     882,
     882,
     882,
     882,
     882,
     882,
     882,
     844.388,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841,
     841
    ]
   },
   {
    "field": "h:entities",
    "unit": "count",
    "step_s": 1,
    "t": [
     0,
     1,
     2,
     3,
     4,
     5,
     6,
     7,
     8,
     9,
     10,
     11,
     12,
     13,
     14,
     15,
     16,
     17,
     18,
     19,
     20,
     21,
     22,
     23,
     24,
     25,
     26,
     27,
     28,
     29,
     30,
     31,
     32,
     33,
     34,
     35,
     36,
     37,
     38,
     39,
     40,
     41,
     42,
     43,
     44
    ],
    "values": [
     null,
     291,
     291,
     291,
     291,
     291,
     289.165,
     289,
     289,
     289,
     288.083,
     290.752,
     291,
     291,
     291,
     291,
     291,
     293.752,
     293.083,
     293,
     293,
     293,
     293,
     295.752,
     296,
     296,
     296,
     296,
     296,
     296,
     294.165,
     292.165,
     292.917,
     294.835,
     294.083,
     294,
     294.917,
     294.083,
     294,
     296.752,
     296.083,
     293.248,
     292.083,
     294.752,
     295
    ]
   }
  ],
  "trailing": [
   20,
   20,
   20
  ],
  "spans": [
   {
    "id": "s:level_overworld",
    "name": "level_overworld",
    "parent": null,
    "values": [
     5.829
    ],
    "cpu": [
     5.829
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tick_mobs",
    "name": "tick_mobs",
    "parent": "s:level_overworld",
    "values": [
     3.089
    ],
    "cpu": [
     3.089
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tick_creature_entities",
    "name": "tick_creature_entities",
    "parent": "s:tick_mobs",
    "values": [
     2.627
    ],
    "cpu": [
     2.627
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:creature_loop",
    "name": "creature_loop",
    "parent": "s:tick_creature_entities",
    "values": [
     2.339
    ],
    "cpu": [
     2.339
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tick_blocks",
    "name": "tick_blocks",
    "parent": "s:level_overworld",
    "values": [
     0.876
    ],
    "cpu": [
     0.876
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tick_lan",
    "name": "tick_lan",
    "parent": "s:level_overworld",
    "values": [
     0.757
    ],
    "cpu": [
     0.757
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:random_ticks",
    "name": "random_ticks",
    "parent": "s:tick_blocks",
    "values": [
     0.561
    ],
    "cpu": [
     0.561
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:spawn_natural",
    "name": "spawn_natural",
    "parent": "s:tick_mobs",
    "values": [
     0.396
    ],
    "cpu": [
     0.396
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:lan_entity_states",
    "name": "lan_entity_states",
    "parent": "s:tick_lan",
    "values": [
     0.362
    ],
    "cpu": [
     0.362
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:sn_attempts",
    "name": "sn_attempts",
    "parent": "s:spawn_natural",
    "values": [
     0.305
    ],
    "cpu": [
     0.305
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tick_items",
    "name": "tick_items",
    "parent": "s:level_overworld",
    "values": [
     0.288
    ],
    "cpu": [
     0.288
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tb_entity_inside",
    "name": "tb_entity_inside",
    "parent": "s:tick_blocks",
    "values": [
     0.237
    ],
    "cpu": [
     0.237
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:creature_prepare",
    "name": "creature_prepare",
    "parent": "s:tick_creature_entities",
    "values": [
     0.205
    ],
    "cpu": [
     0.205
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tick_carts",
    "name": "tick_carts",
    "parent": "s:level_overworld",
    "values": [
     0.159
    ],
    "cpu": [
     0.159
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:lan_tick_level",
    "name": "lan_tick_level",
    "parent": "s:tick_lan",
    "values": [
     0.159
    ],
    "cpu": [
     0.159
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tick_sculk_entities",
    "name": "tick_sculk_entities",
    "parent": "s:level_overworld",
    "values": [
     0.131
    ],
    "cpu": [
     0.131
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:sculk_entity_events",
    "name": "sculk_entity_events",
    "parent": "s:tick_sculk_entities",
    "values": [
     0.12
    ],
    "cpu": [
     0.12
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:lan_guests",
    "name": "lan_guests",
    "parent": "s:tick_lan",
    "values": [
     0.113
    ],
    "cpu": [
     0.113
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:lan_track_entities",
    "name": "lan_track_entities",
    "parent": "s:lan_tick_level",
    "values": [
     0.099
    ],
    "cpu": [
     0.099
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:entity_portals",
    "name": "entity_portals",
    "parent": "s:level_overworld",
    "values": [
     0.093
    ],
    "cpu": [
     0.093
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tick_block_entities",
    "name": "tick_block_entities",
    "parent": "s:level_overworld",
    "values": [
     0.073
    ],
    "cpu": [
     0.073
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:streaming",
    "name": "streaming",
    "parent": "s:level_overworld",
    "values": [
     0.057
    ],
    "cpu": [
     0.057
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:lan_chunks",
    "name": "lan_chunks",
    "parent": "s:lan_tick_level",
    "values": [
     0.052
    ],
    "cpu": [
     0.052
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tb_chunks",
    "name": "tb_chunks",
    "parent": "s:tick_blocks",
    "values": [
     0.05
    ],
    "cpu": [
     0.05
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:sn_state",
    "name": "sn_state",
    "parent": "s:spawn_natural",
    "values": [
     0.042
    ],
    "cpu": [
     0.042
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:creature_villagers",
    "name": "creature_villagers",
    "parent": "s:tick_creature_entities",
    "values": [
     0.03
    ],
    "cpu": [
     0.03
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tick_hud",
    "name": "tick_hud",
    "parent": "s:level_overworld",
    "values": [
     0.024
    ],
    "cpu": [
     0.024
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:sn_candidates",
    "name": "sn_candidates",
    "parent": "s:spawn_natural",
    "values": [
     0.023
    ],
    "cpu": [
     0.023
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tick_lan_packets",
    "name": "tick_lan_packets",
    "parent": "s:level_overworld",
    "values": [
     0.022
    ],
    "cpu": [
     0.022
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:creature_events",
    "name": "creature_events",
    "parent": "s:tick_creature_entities",
    "values": [
     0.022
    ],
    "cpu": [
     0.022
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tick_particles",
    "name": "tick_particles",
    "parent": "s:level_overworld",
    "values": [
     0.021
    ],
    "cpu": [
     0.021
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tick_block_entity_anims",
    "name": "tick_block_entity_anims",
    "parent": "s:level_overworld",
    "values": [
     0.019
    ],
    "cpu": [
     0.019
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:creature_retain",
    "name": "creature_retain",
    "parent": "s:tick_creature_entities",
    "values": [
     0.017
    ],
    "cpu": [
     0.017
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:sn_materialize",
    "name": "sn_materialize",
    "parent": "s:spawn_natural",
    "values": [
     0.014
    ],
    "cpu": [
     0.014
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:postprocess_chunks",
    "name": "postprocess_chunks",
    "parent": "s:level_overworld",
    "values": [
     0.013
    ],
    "cpu": [
     0.013
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:lan_events",
    "name": "lan_events",
    "parent": "s:tick_lan_packets",
    "values": [
     0.012
    ],
    "cpu": [
     0.012
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tick_local_hand",
    "name": "tick_local_hand",
    "parent": "s:level_overworld",
    "values": [
     0.009
    ],
    "cpu": [
     0.009
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:tb_scheduled",
    "name": "tb_scheduled",
    "parent": "s:tick_blocks",
    "values": [
     0.009
    ],
    "cpu": [
     0.009
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:creature_systems",
    "name": "creature_systems",
    "parent": "s:tick_creature_entities",
    "values": [
     0.007
    ],
    "cpu": [
     0.007
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   },
   {
    "id": "s:sculk_step_on",
    "name": "sculk_step_on",
    "parent": "s:tick_sculk_entities",
    "values": [
     0.007
    ],
    "cpu": [
     0.007
    ],
    "waited": [
     0
    ],
    "seen": [
     886
    ]
   }
  ],
  "hot": [
   {
    "id": "f:kq3h1qx3",
    "name": "PalettedContainer::get",
    "full": "horizon_core::paletted::PalettedContainer::get",
    "file": null,
    "line": null,
    "self": 0.582,
    "total": 0.582,
    "self_pct": 10.386,
    "total_pct": 10.386,
    "samples": 129
   },
   {
    "id": "f:prmwv9hs",
    "name": "RandomTickSections::next_masked",
    "full": "horizon_client_lib::random_tick_sections::RandomTickSections::next_masked",
    "file": null,
    "line": null,
    "self": 0.203,
    "total": 0.275,
    "self_pct": 3.623,
    "total_pct": 4.911,
    "samples": 61
   },
   {
    "id": "f:kqnrkqat",
    "name": "ai::tick",
    "full": "horizon_client_lib::mobs::ai::tick",
    "file": null,
    "line": null,
    "self": 0.199,
    "total": 2.122,
    "self_pct": 3.543,
    "total_pct": 37.842,
    "samples": 470
   },
   {
    "id": "f:k61z3gs5",
    "name": "__memmove_avx_unaligned_erms",
    "full": "__memmove_avx_unaligned_erms",
    "file": null,
    "line": null,
    "self": 0.194,
    "total": 0.194,
    "self_pct": 3.462,
    "total_pct": 3.462,
    "samples": 43
   },
   {
    "id": "f:ne9h54ke",
    "name": "_mi_page_malloc_zero",
    "full": "_mi_page_malloc_zero",
    "file": null,
    "line": null,
    "self": 0.194,
    "total": 0.194,
    "self_pct": 3.462,
    "total_pct": 3.462,
    "samples": 43
   },
   {
    "id": "f:vv8h8aba",
    "name": "Behaviours::tick_chunk_random_masked",
    "full": "horizon_world::level::Behaviours::tick_chunk_random_masked",
    "file": null,
    "line": null,
    "self": 0.194,
    "total": 0.609,
    "self_pct": 3.462,
    "total_pct": 10.87,
    "samples": 135
   },
   {
    "id": "f:kd5efx0c",
    "name": "World::block",
    "full": "horizon_client_render::world::World::block",
    "file": null,
    "line": null,
    "self": 0.181,
    "total": 0.185,
    "self_pct": 3.221,
    "total_pct": 3.301,
    "samples": 41
   },
   {
    "id": "f:q0p6pgdz",
    "name": "ptr::drop_glue",
    "full": "core::ptr::drop_glue",
    "file": null,
    "line": null,
    "self": 0.176,
    "total": 0.221,
    "self_pct": 3.14,
    "total_pct": 3.945,
    "samples": 49
   },
   {
    "id": "f:kc0xgby3",
    "name": "resolve_world_movement::{closure}",
    "full": "horizon_world::world_movement::resolve_world_movement::{closure}",
    "file": null,
    "line": null,
    "self": 0.153,
    "total": 0.447,
    "self_pct": 2.738,
    "total_pct": 7.971,
    "samples": 99
   },
   {
    "id": "f:9agx3qe8",
    "name": "Mob::awake_dims_in",
    "full": "horizon_client_lib::mobs::Mob::awake_dims_in",
    "file": null,
    "line": null,
    "self": 0.072,
    "total": 0.072,
    "self_pct": 1.288,
    "total_pct": 1.288,
    "samples": 16
   },
   {
    "id": "f:369375at",
    "name": "SeenGrid::candidates",
    "full": "horizon_client_lib::mobs::seen_grid::SeenGrid::candidates",
    "file": null,
    "line": null,
    "self": 0.072,
    "total": 0.099,
    "self_pct": 1.288,
    "total_pct": 1.771,
    "samples": 22
   },
   {
    "id": "f:e6dj9wvr",
    "name": "PalettedContainer::maybe_has",
    "full": "horizon_core::paletted::PalettedContainer::maybe_has",
    "file": null,
    "line": null,
    "self": 0.068,
    "total": 0.068,
    "self_pct": 1.208,
    "total_pct": 1.208,
    "samples": 15
   }
  ],
  "worst": [
   {
    "id": "k:1",
    "t": 0.181,
    "wall_ms": 96.798,
    "cpu_ms": 96.268,
    "before_capture": false,
    "complete": true,
    "spans": [
     [
      "level_overworld",
      0,
      96.743,
      96.743
     ],
     [
      "tick_lan_packets",
      1,
      9.274,
      9.274
     ],
     [
      "lan_events",
      2,
      9.259,
      9.259
     ],
     [
      "lan_poll",
      2,
      0.007,
      0.007
     ],
     [
      "lan_drain",
      2,
      0.004,
      0.004
     ],
     [
      "tick_hud",
      1,
      7.935,
      7.935
     ],
     [
      "tick_sculk_entities",
      1,
      3.452,
      3.452
     ],
     [
      "sculk_prepare",
      2,
      3.445,
      3.445
     ],
     [
      "sculk_entity_events",
      2,
      0.001,
      0.001
     ],
     [
      "sculk_wardens",
      2,
      0.001,
      0.001
     ],
     [
      "sculk_step_on",
      2,
      0,
      0
     ],
     [
      "tick_mobs",
      1,
      1.703,
      1.703
     ],
     [
      "tick_creature_entities",
      2,
      0.021,
      0.021
     ],
     [
      "creature_villagers",
      3,
      0.006,
      0.006
     ],
     [
      "creature_systems",
      3,
      0.004,
      0.004
     ],
     [
      "creature_prepare",
      3,
      0.004,
      0.004
     ],
     [
      "creature_projectiles",
      3,
      0.001,
      0.001
     ],
     [
      "creature_retain",
      3,
      0.001,
      0.001
     ],
     [
      "creature_events",
      3,
      0.001,
      0.001
     ],
     [
      "creature_outbox",
      3,
      0,
      0
     ],
     [
      "creature_loop",
      3,
      0,
      0
     ],
     [
      "spawn_natural",
      2,
      0.013,
      0.013
     ],
     [
      "sn_candidates",
      3,
      0.004,
      0.004
     ],
     [
      "sn_level",
      3,
      0.004,
      0.004
     ],
     [
      "sn_finalize",
      3,
      0.001,
      0.001
     ],
     [
      "sn_state",
      3,
      0.001,
      0.001
     ],
     [
      "sn_attempts",
      3,
      0.001,
      0.001
     ],
     [
      "sn_categories",
      3,
      0.001,
      0.001
     ],
     [
      "sn_materialize",
      3,
      0,
      0
     ],
     [
      "sn_shuffle",
      3,
      0,
      0
     ],
     [
      "sn_inhabited",
      3,
      0,
      0
     ],
     [
      "tick_block_entities",
      1,
      1.165,
      1.165
     ],
     [
      "streaming",
      1,
      0.393,
      0.393
     ],
     [
      "tick_lan",
      1,
      0.247,
      0.247
     ],
     [
      "lan_tick_level",
      2,
      0.194,
      0.194
     ],
     [
      "lan_player_state",
      3,
      0.155,
      0.155
     ],
     [
      "lan_chunks",
      3,
      0.025,
      0.025
     ],
     [
      "lan_track_entities",
      3,
      0.012,
      0.012
     ],
     [
      "lan_containers",
      3,
      0,
      0
     ],
     [
      "lan_block_changes",
      3,
      0,
      0
     ],
     [
      "lan_sync_world",
      3,
      0,
      0
     ],
     [
      "lan_guests",
      2,
      0.009,
      0.009
     ],
     [
      "lan_entity_states",
      2,
      0.004,
      0.004
     ],
     [
      "lan_process_events",
      2,
      0.001,
      0.001
     ],
     [
      "lan_end_tick",
      2,
      0,
      0
     ],
     [
      "tick_blocks",
      1,
      0.036,
      0.036
     ],
     [
      "tb_entity_inside",
      2,
      0.016,
      0.016
     ],
     [
      "tb_scheduled",
      2,
      0.011,
      0.011
     ],
     [
      "tb_tickets",
      2,
      0.002,
      0.002
     ],
     [
      "tb_fluids",
      2,
      0.001,
      0.001
     ],
     [
      "tb_block_events",
      2,
      0.001,
      0.001
     ],
     [
      "tb_chunks",
      2,
      0.001,
      0.001
     ],
     [
      "tb_broadcast",
      2,
      0.001,
      0.001
     ],
     [
      "tb_pistons",
      2,
      0,
      0
     ],
     [
      "random_ticks",
      2,
      0,
      0
     ],
     [
      "tb_level_events",
      2,
      0,
      0
     ],
     [
      "tick_local_hand",
      1,
      0.018,
      0.018
     ],
     [
      "spawn_loaded_entities",
      1,
      0.005,
      0.005
     ],
     [
      "tick_items",
      1,
      0.004,
      0.004
     ],
     [
      "tick_boats",
      1,
      0.004,
      0.004
     ],
     [
      "tick_hand",
      1,
      0.003,
      0.003
     ],
     [
      "tick_maps",
      1,
      0.003,
      0.003
     ],
     [
      "tick_fishing",
      1,
      0.003,
      0.003
     ],
     [
      "tick_particles",
      1,
      0.003,
      0.003
     ],
     [
      "tick_moving_pistons",
      1,
      0.003,
      0.003
     ],
     [
      "entity_portals",
      1,
      0.002,
      0.002
     ],
     [
      "tick_effects",
      1,
      0.001,
      0.001
     ],
     [
      "tick_block_entity_anims",
      1,
      0.001,
      0.001
     ],
     [
      "tick_sculk_blocks",
      1,
      0.001,
      0.001
     ],
     [
      "with_entities",
      1,
      0.001,
      0.001
     ],
     [
      "lan_note_joined",
      1,
      0.001,
      0.001
     ],
     [
      "tick_bolts",
      1,
      0.001,
      0.001
     ],
     [
      "tick_carts",
      1,
      0,
      0
     ],
     [
      "tick_end",
      1,
      0,
      0
     ],
     [
      "tick_sleep",
      1,
      0,
      0
     ],
     [
      "poll_remote",
      1,
      0,
      0
     ],
     [
      "tick_ominous_item_spawners",
      1,
      0,
      0
     ],
     [
      "tick_experience",
      1,
      0,
      0
     ],
     [
      "tick_item_frames",
      1,
      0,
      0
     ],
     [
      "postprocess_chunks",
      1,
      0,
      0
     ],
     [
      "tick_thunder",
      1,
      0,
      0
     ],
     [
      "end_gateways",
      1,
      0,
      0
     ],
     [
      "tick_armor_stands",
      1,
      0,
      0
     ],
     [
      "recheck_golem_openers",
      1,
      0,
      0
     ],
     [
      "tick_cushions",
      1,
      0,
      0
     ],
     [
      "tick_tridents",
      1,
      0,
      0
     ],
     [
      "tick_lecterns",
      1,
      0,
      0
     ],
     [
      "tick_rockets",
      1,
      0,
      0
     ],
     [
      "unload_entities",
      1,
      0,
      0
     ],
     [
      "tick_using",
      1,
      0,
      0
     ],
     [
      "tick_paintings",
      1,
      0,
      0
     ],
     [
      "touch_arrows",
      1,
      0,
      0
     ]
    ]
   },
   {
    "id": "k:300",
    "t": 15.086,
    "wall_ms": 53.767,
    "cpu_ms": 30.937,
    "before_capture": false,
    "complete": true,
    "spans": [
     [
      "level_overworld",
      0,
      5.953,
      5.953
     ],
     [
      "tick_mobs",
      1,
      3.531,
      3.531
     ],
     [
      "tick_creature_entities",
      2,
      3.192,
      3.192
     ],
     [
      "creature_loop",
      3,
      3.037,
      3.037
     ],
     [
      "creature_prepare",
      3,
      0.115,
      0.115
     ],
     [
      "creature_villagers",
      3,
      0.015,
      0.015
     ],
     [
      "creature_events",
      3,
      0.012,
      0.012
     ],
     [
      "creature_retain",
      3,
      0.006,
      0.006
     ],
     [
      "creature_systems",
      3,
      0.004,
      0.004
     ],
     [
      "creature_outbox",
      3,
      0.001,
      0.001
     ],
     [
      "creature_projectiles",
      3,
      0.001,
      0.001
     ],
     [
      "spawn_natural",
      2,
      0.295,
      0.295
     ],
     [
      "sn_attempts",
      3,
      0.238,
      0.238
     ],
     [
      "sn_state",
      3,
      0.024,
      0.024
     ],
     [
      "sn_candidates",
      3,
      0.017,
      0.017
     ],
     [
      "sn_materialize",
      3,
      0.009,
      0.009
     ],
     [
      "sn_level",
      3,
      0.004,
      0.004
     ],
     [
      "sn_inhabited",
      3,
      0.002,
      0.002
     ],
     [
      "sn_shuffle",
      3,
      0.001,
      0.001
     ],
     [
      "sn_finalize",
      3,
      0.001,
      0.001
     ],
     [
      "sn_categories",
      3,
      0.001,
      0.001
     ],
     [
      "tick_lan",
      1,
      1.197,
      1.197
     ],
     [
      "lan_entity_states",
      2,
      0.517,
      0.517
     ],
     [
      "lan_tick_level",
      2,
      0.257,
      0.257
     ],
     [
      "lan_track_entities",
      3,
      0.239,
      0.239
     ],
     [
      "lan_sync_world",
      3,
      0.005,
      0.005
     ],
     [
      "lan_player_state",
      3,
      0.002,
      0.002
     ],
     [
      "lan_chunks",
      3,
      0.002,
      0.002
     ],
     [
      "lan_block_changes",
      3,
      0.001,
      0.001
     ],
     [
      "lan_containers",
      3,
      0.001,
      0.001
     ],
     [
      "lan_guests",
      2,
      0.254,
      0.254
     ],
     [
      "lan_process_events",
      2,
      0.001,
      0.001
     ],
     [
      "lan_end_tick",
      2,
      0,
      0
     ],
     [
      "tick_blocks",
      1,
      0.602,
      0.602
     ],
     [
      "random_ticks",
      2,
      0.391,
      0.391
     ],
     [
      "tb_entity_inside",
      2,
      0.174,
      0.174
     ],
     [
      "tb_chunks",
      2,
      0.021,
      0.021
     ],
     [
      "tb_scheduled",
      2,
      0.007,
      0.007
     ],
     [
      "tb_block_events",
      2,
      0.002,
      0.002
     ],
     [
      "tb_tickets",
      2,
      0.002,
      0.002
     ],
     [
      "tb_fluids",
      2,
      0.002,
      0.002
     ],
     [
      "tb_broadcast",
      2,
      0.001,
      0.001
     ],
     [
      "tb_pistons",
      2,
      0.001,
      0.001
     ],
     [
      "tb_level_events",
      2,
      0,
      0
     ],
     [
      "tick_items",
      1,
      0.174,
      0.174
     ],
     [
      "tick_carts",
      1,
      0.114,
      0.114
     ],
     [
      "tick_sculk_entities",
      1,
      0.099,
      0.099
     ],
     [
      "sculk_entity_events",
      2,
      0.094,
      0.094
     ],
     [
      "sculk_step_on",
      2,
      0.005,
      0.005
     ],
     [
      "sculk_wardens",
      2,
      0,
      0
     ],
     [
      "sculk_prepare",
      2,
      0,
      0
     ],
     [
      "entity_portals",
      1,
      0.052,
      0.052
     ],
     [
      "tick_block_entities",
      1,
      0.041,
      0.041
     ],
     [
      "streaming",
      1,
      0.025,
      0.025
     ],
     [
      "tick_hud",
      1,
      0.01,
      0.01
     ],
     [
      "postprocess_chunks",
      1,
      0.01,
      0.01
     ],
     [
      "tick_lan_packets",
      1,
      0.008,
      0.008
     ],
     [
      "lan_poll",
      2,
      0.002,
      0.002
     ],
     [
      "lan_drain",
      2,
      0.001,
      0.001
     ],
     [
      "lan_events",
      2,
      0.001,
      0.001
     ],
     [
      "tick_local_hand",
      1,
      0.006,
      0.006
     ],
     [
      "tick_moving_pistons",
      1,
      0.006,
      0.006
     ],
     [
      "tick_particles",
      1,
      0.005,
      0.005
     ],
     [
      "tick_hand",
      1,
      0.004,
      0.004
     ],
     [
      "tick_boats",
      1,
      0.004,
      0.004
     ],
     [
      "tick_effects",
      1,
      0.001,
      0.001
     ],
     [
      "tick_sculk_blocks",
      1,
      0.001,
      0.001
     ],
     [
      "tick_block_entity_anims",
      1,
      0.001,
      0.001
     ],
     [
      "tick_end",
      1,
      0.001,
      0.001
     ],
     [
      "lan_note_joined",
      1,
      0.001,
      0.001
     ],
     [
      "with_entities",
      1,
      0.001,
      0.001
     ],
     [
      "tick_cushions",
      1,
      0.001,
      0.001
     ],
     [
      "spawn_loaded_entities",
      1,
      0.001,
      0.001
     ],
     [
      "tick_sleep",
      1,
      0,
      0
     ],
     [
      "tick_experience",
      1,
      0,
      0
     ],
     [
      "tick_bolts",
      1,
      0,
      0
     ],
     [
      "poll_remote",
      1,
      0,
      0
     ],
     [
      "tick_item_frames",
      1,
      0,
      0
     ],
     [
      "tick_ominous_item_spawners",
      1,
      0,
      0
     ],
     [
      "tick_fishing",
      1,
      0,
      0
     ],
     [
      "tick_maps",
      1,
      0,
      0
     ],
     [
      "end_gateways",
      1,
      0,
      0
     ],
     [
      "tick_lecterns",
      1,
      0,
      0
     ],
     [
      "tick_armor_stands",
      1,
      0,
      0
     ],
     [
      "tick_thunder",
      1,
      0,
      0
     ],
     [
      "tick_rockets",
      1,
      0,
      0
     ],
     [
      "tick_tridents",
      1,
      0,
      0
     ],
     [
      "recheck_golem_openers",
      1,
      0,
      0
     ],
     [
      "tick_using",
      1,
      0,
      0
     ],
     [
      "unload_entities",
      1,
      0,
      0
     ],
     [
      "touch_arrows",
      1,
      0,
      0
     ],
     [
      "tick_paintings",
      1,
      0,
      0
     ]
    ]
   },
   {
    "id": "k:880",
    "t": 44.063,
    "wall_ms": 27.781,
    "cpu_ms": 8.865,
    "before_capture": false,
    "complete": true,
    "spans": [
     [
      "level_overworld",
      0,
      27.736,
      27.736
     ],
     [
      "tick_mobs",
      1,
      17.026,
      17.026
     ],
     [
      "tick_creature_entities",
      2,
      16.26,
      16.26
     ],
     [
      "creature_loop",
      3,
      9.664,
      9.664
     ],
     [
      "creature_prepare",
      3,
      6.465,
      6.465
     ],
     [
      "creature_villagers",
      3,
      0.048,
      0.048
     ],
     [
      "creature_events",
      3,
      0.033,
      0.033
     ],
     [
      "creature_retain",
      3,
      0.028,
      0.028
     ],
     [
      "creature_systems",
      3,
      0.011,
      0.011
     ],
     [
      "creature_outbox",
      3,
      0.005,
      0.005
     ],
     [
      "creature_projectiles",
      3,
      0.002,
      0.002
     ],
     [
      "spawn_natural",
      2,
      0.664,
      0.664
     ],
     [
      "sn_attempts",
      3,
      0.514,
      0.514
     ],
     [
      "sn_state",
      3,
      0.057,
      0.057
     ],
     [
      "sn_candidates",
      3,
      0.043,
      0.043
     ],
     [
      "sn_materialize",
      3,
      0.019,
      0.019
     ],
     [
      "sn_finalize",
      3,
      0.015,
      0.015
     ],
     [
      "sn_level",
      3,
      0.009,
      0.009
     ],
     [
      "sn_inhabited",
      3,
      0.004,
      0.004
     ],
     [
      "sn_shuffle",
      3,
      0.001,
      0.001
     ],
     [
      "sn_categories",
      3,
      0.001,
      0.001
     ],
     [
      "tick_lan",
      1,
      6.382,
      6.382
     ],
     [
      "lan_entity_states",
      2,
      0.523,
      0.523
     ],
     [
      "lan_guests",
      2,
      0.274,
      0.274
     ],
     [
      "lan_tick_level",
      2,
      0.157,
      0.157
     ],
     [
      "lan_track_entities",
      3,
      0.144,
      0.144
     ],
     [
      "lan_player_state",
      3,
      0.004,
      0.004
     ],
     [
      "lan_sync_world",
      3,
      0.003,
      0.003
     ],
     [
      "lan_block_changes",
      3,
      0.001,
      0.001
     ],
     [
      "lan_chunks",
      3,
      0.001,
      0.001
     ],
     [
      "lan_containers",
      3,
      0.001,
      0.001
     ],
     [
      "lan_process_events",
      2,
      0.002,
      0.002
     ],
     [
      "lan_end_tick",
      2,
      0,
      0
     ],
     [
      "tick_sculk_entities",
      1,
      1.937,
      1.937
     ],
     [
      "sculk_entity_events",
      2,
      1.915,
      1.915
     ],
     [
      "sculk_step_on",
      2,
      0.017,
      0.017
     ],
     [
      "sculk_wardens",
      2,
      0.001,
      0.001
     ],
     [
      "sculk_prepare",
      2,
      0.001,
      0.001
     ],
     [
      "tick_blocks",
      1,
      1.117,
      1.117
     ],
     [
      "random_ticks",
      2,
      0.737,
      0.737
     ],
     [
      "tb_entity_inside",
      2,
      0.3,
      0.3
     ],
     [
      "tb_chunks",
      2,
      0.048,
      0.048
     ],
     [
      "tb_scheduled",
      2,
      0.012,
      0.012
     ],
     [
      "tb_block_events",
      2,
      0.005,
      0.005
     ],
     [
      "tb_fluids",
      2,
      0.004,
      0.004
     ],
     [
      "tb_tickets",
      2,
      0.004,
      0.004
     ],
     [
      "tb_broadcast",
      2,
      0.002,
      0.002
     ],
     [
      "tb_pistons",
      2,
      0.001,
      0.001
     ],
     [
      "tb_level_events",
      2,
      0,
      0
     ],
     [
      "tick_items",
      1,
      0.376,
      0.376
     ],
     [
      "tick_block_entities",
      1,
      0.254,
      0.254
     ],
     [
      "tick_carts",
      1,
      0.2,
      0.2
     ],
     [
      "entity_portals",
      1,
      0.138,
      0.138
     ],
     [
      "streaming",
      1,
      0.051,
      0.051
     ],
     [
      "tick_hud",
      1,
      0.017,
      0.017
     ],
     [
      "postprocess_chunks",
      1,
      0.016,
      0.016
     ],
     [
      "tick_particles",
      1,
      0.015,
      0.015
     ],
     [
      "tick_lan_packets",
      1,
      0.014,
      0.014
     ],
     [
      "lan_poll",
      2,
      0.005,
      0.005
     ],
     [
      "lan_drain",
      2,
      0.003,
      0.003
     ],
     [
      "lan_events",
      2,
      0.002,
      0.002
     ],
     [
      "tick_moving_pistons",
      1,
      0.012,
      0.012
     ],
     [
      "tick_hand",
      1,
      0.012,
      0.012
     ],
     [
      "tick_local_hand",
      1,
      0.011,
      0.011
     ],
     [
      "lan_note_joined",
      1,
      0.01,
      0.01
     ],
     [
      "tick_boats",
      1,
      0.007,
      0.007
     ],
     [
      "tick_sleep",
      1,
      0.003,
      0.003
     ],
     [
      "tick_sculk_blocks",
      1,
      0.002,
      0.002
     ],
     [
      "tick_block_entity_anims",
      1,
      0.002,
      0.002
     ],
     [
      "tick_effects",
      1,
      0.002,
      0.002
     ],
     [
      "tick_end",
      1,
      0.001,
      0.001
     ],
     [
      "with_entities",
      1,
      0.001,
      0.001
     ],
     [
      "tick_cushions",
      1,
      0.001,
      0.001
     ],
     [
      "spawn_loaded_entities",
      1,
      0.001,
      0.001
     ],
     [
      "tick_bolts",
      1,
      0.001,
      0.001
     ],
     [
      "tick_experience",
      1,
      0.001,
      0.001
     ],
     [
      "tick_ominous_item_spawners",
      1,
      0.001,
      0.001
     ],
     [
      "poll_remote",
      1,
      0.001,
      0.001
     ],
     [
      "tick_lecterns",
      1,
      0.001,
      0.001
     ],
     [
      "tick_armor_stands",
      1,
      0.001,
      0.001
     ],
     [
      "tick_maps",
      1,
      0.001,
      0.001
     ],
     [
      "tick_item_frames",
      1,
      0,
      0
     ],
     [
      "end_gateways",
      1,
      0,
      0
     ],
     [
      "tick_thunder",
      1,
      0,
      0
     ],
     [
      "tick_rockets",
      1,
      0,
      0
     ],
     [
      "tick_using",
      1,
      0,
      0
     ],
     [
      "tick_fishing",
      1,
      0,
      0
     ],
     [
      "recheck_golem_openers",
      1,
      0,
      0
     ],
     [
      "touch_arrows",
      1,
      0,
      0
     ],
     [
      "tick_tridents",
      1,
      0,
      0
     ],
     [
      "unload_entities",
      1,
      0,
      0
     ],
     [
      "tick_paintings",
      1,
      0,
      0
     ]
    ]
   },
   {
    "id": "k:4",
    "t": 0.255,
    "wall_ms": 22.271,
    "cpu_ms": 21.514,
    "before_capture": false,
    "complete": true,
    "spans": [
     [
      "level_overworld",
      0,
      22.235,
      22.235
     ],
     [
      "tick_blocks",
      1,
      11.505,
      11.505
     ],
     [
      "random_ticks",
      2,
      10.728,
      10.728
     ],
     [
      "tb_tickets",
      2,
      0.658,
      0.658
     ],
     [
      "tb_entity_inside",
      2,
      0.065,
      0.065
     ],
     [
      "tb_chunks",
      2,
      0.037,
      0.037
     ],
     [
      "tb_scheduled",
      2,
      0.005,
      0.005
     ],
     [
      "tb_block_events",
      2,
      0.005,
      0.005
     ],
     [
      "tb_broadcast",
      2,
      0.001,
      0.001
     ],
     [
      "tb_fluids",
      2,
      0.001,
      0.001
     ],
     [
      "tb_pistons",
      2,
      0.001,
      0.001
     ],
     [
      "tb_level_events",
      2,
      0,
      0
     ],
     [
      "tick_mobs",
      1,
      4.659,
      4.659
     ],
     [
      "spawn_natural",
      2,
      3.03,
      3.03
     ],
     [
      "sn_attempts",
      3,
      2.958,
      2.958
     ],
     [
      "sn_state",
      3,
      0.033,
      0.033
     ],
     [
      "sn_candidates",
      3,
      0.02,
      0.02
     ],
     [
      "sn_materialize",
      3,
      0.008,
      0.008
     ],
     [
      "sn_level",
      3,
      0.004,
      0.004
     ],
     [
      "sn_inhabited",
      3,
      0.003,
      0.003
     ],
     [
      "sn_shuffle",
      3,
      0.001,
      0.001
     ],
     [
      "sn_finalize",
      3,
      0.001,
      0.001
     ],
     [
      "sn_categories",
      3,
      0.001,
      0.001
     ],
     [
      "tick_creature_entities",
      2,
      1.591,
      1.591
     ],
     [
      "creature_loop",
      3,
      1.438,
      1.438
     ],
     [
      "creature_prepare",
      3,
      0.117,
      0.117
     ],
     [
      "creature_villagers",
      3,
      0.016,
      0.016
     ],
     [
      "creature_retain",
      3,
      0.006,
      0.006
     ],
     [
      "creature_events",
      3,
      0.006,
      0.006
     ],
     [
      "creature_systems",
      3,
      0.004,
      0.004
     ],
     [
      "creature_outbox",
      3,
      0.001,
      0.001
     ],
     [
      "creature_projectiles",
      3,
      0.001,
      0.001
     ],
     [
      "tick_lan",
      1,
      3.475,
      3.475
     ],
     [
      "lan_tick_level",
      2,
      2.846,
      2.846
     ],
     [
      "lan_chunks",
      3,
      2.463,
      2.463
     ],
     [
      "lan_track_entities",
      3,
      0.377,
      0.377
     ],
     [
      "lan_player_state",
      3,
      0.003,
      0.003
     ],
     [
      "lan_block_changes",
      3,
      0.001,
      0.001
     ],
     [
      "lan_containers",
      3,
      0,
      0
     ],
     [
      "lan_sync_world",
      3,
      0,
      0
     ],
     [
      "lan_entity_states",
      2,
      0.463,
      0.463
     ],
     [
      "lan_guests",
      2,
      0.096,
      0.096
     ],
     [
      "lan_process_events",
      2,
      0.001,
      0.001
     ],
     [
      "lan_end_tick",
      2,
      0,
      0
     ],
     [
      "spawn_loaded_entities",
      1,
      1.626,
      1.626
     ],
     [
      "tick_block_entities",
      1,
      0.527,
      0.527
     ],
     [
      "entity_portals",
      1,
      0.184,
      0.184
     ],
     [
      "tick_sculk_entities",
      1,
      0.083,
      0.083
     ],
     [
      "sculk_entity_events",
      2,
      0.077,
      0.077
     ],
     [
      "sculk_step_on",
      2,
      0.005,
      0.005
     ],
     [
      "sculk_wardens",
      2,
      0.001,
      0.001
     ],
     [
      "sculk_prepare",
      2,
      0,
      0
     ],
     [
      "tick_items",
      1,
      0.033,
      0.033
     ],
     [
      "tick_lan_packets",
      1,
      0.028,
      0.028
     ],
     [
      "lan_drain",
      2,
      0.018,
      0.018
     ],
     [
      "lan_events",
      2,
      0.004,
      0.004
     ],
     [
      "lan_poll",
      2,
      0.004,
      0.004
     ],
     [
      "tick_hud",
      1,
      0.011,
      0.011
     ],
     [
      "tick_local_hand",
      1,
      0.007,
      0.007
     ],
     [
      "streaming",
      1,
      0.006,
      0.006
     ],
     [
      "postprocess_chunks",
      1,
      0.006,
      0.006
     ],
     [
      "tick_moving_pistons",
      1,
      0.006,
      0.006
     ],
     [
      "tick_particles",
      1,
      0.004,
      0.004
     ],
     [
      "tick_hand",
      1,
      0.003,
      0.003
     ],
     [
      "tick_boats",
      1,
      0.002,
      0.002
     ],
     [
      "tick_carts",
      1,
      0.001,
      0.001
     ],
     [
      "lan_note_joined",
      1,
      0.001,
      0.001
     ],
     [
      "tick_effects",
      1,
      0.001,
      0.001
     ],
     [
      "tick_sculk_blocks",
      1,
      0.001,
      0.001
     ],
     [
      "tick_block_entity_anims",
      1,
      0.001,
      0.001
     ],
     [
      "tick_end",
      1,
      0.001,
      0.001
     ],
     [
      "tick_bolts",
      1,
      0.001,
      0.001
     ],
     [
      "tick_sleep",
      1,
      0,
      0
     ],
     [
      "tick_ominous_item_spawners",
      1,
      0,
      0
     ],
     [
      "tick_maps",
      1,
      0,
      0
     ],
     [
      "poll_remote",
      1,
      0,
      0
     ],
     [
      "tick_item_frames",
      1,
      0,
      0
     ],
     [
      "tick_experience",
      1,
      0,
      0
     ],
     [
      "tick_armor_stands",
      1,
      0,
      0
     ],
     [
      "tick_thunder",
      1,
      0,
      0
     ],
     [
      "end_gateways",
      1,
      0,
      0
     ],
     [
      "recheck_golem_openers",
      1,
      0,
      0
     ],
     [
      "tick_cushions",
      1,
      0,
      0
     ],
     [
      "tick_lecterns",
      1,
      0,
      0
     ],
     [
      "tick_fishing",
      1,
      0,
      0
     ],
     [
      "tick_tridents",
      1,
      0,
      0
     ],
     [
      "tick_using",
      1,
      0,
      0
     ],
     [
      "tick_rockets",
      1,
      0,
      0
     ],
     [
      "tick_paintings",
      1,
      0,
      0
     ],
     [
      "with_entities",
      1,
      0,
      0
     ],
     [
      "touch_arrows",
      1,
      0,
      0
     ],
     [
      "unload_entities",
      1,
      0,
      0
     ]
    ]
   },
   {
    "id": "k:878",
    "t": 43.953,
    "wall_ms": 19.753,
    "cpu_ms": 8.221,
    "before_capture": false,
    "complete": true,
    "spans": [
     [
      "level_overworld",
      0,
      19.711,
      19.711
     ],
     [
      "tick_mobs",
      1,
      16.212,
      16.212
     ],
     [
      "tick_creature_entities",
      2,
      15.485,
      15.485
     ],
     [
      "creature_loop",
      3,
      15.01,
      15.01
     ],
     [
      "creature_prepare",
      3,
      0.359,
      0.359
     ],
     [
      "creature_villagers",
      3,
      0.045,
      0.045
     ],
     [
      "creature_events",
      3,
      0.031,
      0.031
     ],
     [
      "creature_retain",
      3,
      0.021,
      0.021
     ],
     [
      "creature_systems",
      3,
      0.01,
      0.01
     ],
     [
      "creature_outbox",
      3,
      0.004,
      0.004
     ],
     [
      "creature_projectiles",
      3,
      0.002,
      0.002
     ],
     [
      "spawn_natural",
      2,
      0.633,
      0.633
     ],
     [
      "sn_attempts",
      3,
      0.491,
      0.491
     ],
     [
      "sn_state",
      3,
      0.061,
      0.061
     ],
     [
      "sn_candidates",
      3,
      0.042,
      0.042
     ],
     [
      "sn_materialize",
      3,
      0.022,
      0.022
     ],
     [
      "sn_level",
      3,
      0.008,
      0.008
     ],
     [
      "sn_inhabited",
      3,
      0.004,
      0.004
     ],
     [
      "sn_categories",
      3,
      0.002,
      0.002
     ],
     [
      "sn_shuffle",
      3,
      0.001,
      0.001
     ],
     [
      "sn_finalize",
      3,
      0.001,
      0.001
     ],
     [
      "tick_blocks",
      1,
      1.124,
      1.124
     ],
     [
      "random_ticks",
      2,
      0.745,
      0.745
     ],
     [
      "tb_entity_inside",
      2,
      0.301,
      0.301
     ],
     [
      "tb_chunks",
      2,
      0.044,
      0.044
     ],
     [
      "tb_scheduled",
      2,
      0.013,
      0.013
     ],
     [
      "tb_block_events",
      2,
      0.005,
      0.005
     ],
     [
      "tb_fluids",
      2,
      0.004,
      0.004
     ],
     [
      "tb_tickets",
      2,
      0.004,
      0.004
     ],
     [
      "tb_broadcast",
      2,
      0.002,
      0.002
     ],
     [
      "tb_pistons",
      2,
      0.001,
      0.001
     ],
     [
      "tb_level_events",
      2,
      0,
      0
     ],
     [
      "tick_lan",
      1,
      1.012,
      1.012
     ],
     [
      "lan_entity_states",
      2,
      0.496,
      0.496
     ],
     [
      "lan_guests",
      2,
      0.18,
      0.18
     ],
     [
      "lan_tick_level",
      2,
      0.159,
      0.159
     ],
     [
      "lan_track_entities",
      3,
      0.148,
      0.148
     ],
     [
      "lan_player_state",
      3,
      0.005,
      0.005
     ],
     [
      "lan_chunks",
      3,
      0.001,
      0.001
     ],
     [
      "lan_block_changes",
      3,
      0.001,
      0.001
     ],
     [
      "lan_containers",
      3,
      0.001,
      0.001
     ],
     [
      "lan_sync_world",
      3,
      0,
      0
     ],
     [
      "lan_process_events",
      2,
      0.002,
      0.002
     ],
     [
      "lan_end_tick",
      2,
      0,
      0
     ],
     [
      "tick_items",
      1,
      0.358,
      0.358
     ],
     [
      "tick_sculk_entities",
      1,
      0.223,
      0.223
     ],
     [
      "sculk_entity_events",
      2,
      0.21,
      0.21
     ],
     [
      "sculk_step_on",
      2,
      0.011,
      0.011
     ],
     [
      "sculk_wardens",
      2,
      0.001,
      0.001
     ],
     [
      "sculk_prepare",
      2,
      0,
      0
     ],
     [
      "tick_carts",
      1,
      0.215,
      0.215
     ],
     [
      "tick_block_entities",
      1,
      0.152,
      0.152
     ],
     [
      "entity_portals",
      1,
      0.121,
      0.121
     ],
     [
      "streaming",
      1,
      0.054,
      0.054
     ],
     [
      "tick_hud",
      1,
      0.02,
      0.02
     ],
     [
      "postprocess_chunks",
      1,
      0.016,
      0.016
     ],
     [
      "tick_particles",
      1,
      0.014,
      0.014
     ],
     [
      "tick_local_hand",
      1,
      0.013,
      0.013
     ],
     [
      "tick_lan_packets",
      1,
      0.013,
      0.013
     ],
     [
      "lan_poll",
      2,
      0.004,
      0.004
     ],
     [
      "lan_drain",
      2,
      0.002,
      0.002
     ],
     [
      "lan_events",
      2,
      0.002,
      0.002
     ],
     [
      "tick_moving_pistons",
      1,
      0.011,
      0.011
     ],
     [
      "tick_hand",
      1,
      0.008,
      0.008
     ],
     [
      "tick_boats",
      1,
      0.007,
      0.007
     ],
     [
      "lan_note_joined",
      1,
      0.004,
      0.004
     ],
     [
      "tick_block_entity_anims",
      1,
      0.002,
      0.002
     ],
     [
      "tick_effects",
      1,
      0.002,
      0.002
     ],
     [
      "with_entities",
      1,
      0.002,
      0.002
     ],
     [
      "tick_sculk_blocks",
      1,
      0.001,
      0.001
     ],
     [
      "poll_remote",
      1,
      0.001,
      0.001
     ],
     [
      "tick_cushions",
      1,
      0.001,
      0.001
     ],
     [
      "tick_end",
      1,
      0.001,
      0.001
     ],
     [
      "spawn_loaded_entities",
      1,
      0.001,
      0.001
     ],
     [
      "tick_sleep",
      1,
      0.001,
      0.001
     ],
     [
      "recheck_golem_openers",
      1,
      0.001,
      0.001
     ],
     [
      "tick_item_frames",
      1,
      0.001,
      0.001
     ],
     [
      "tick_experience",
      1,
      0.001,
      0.001
     ],
     [
      "end_gateways",
      1,
      0.001,
      0.001
     ],
     [
      "tick_ominous_item_spawners",
      1,
      0.001,
      0.001
     ],
     [
      "tick_maps",
      1,
      0,
      0
     ],
     [
      "tick_fishing",
      1,
      0,
      0
     ],
     [
      "tick_armor_stands",
      1,
      0,
      0
     ],
     [
      "tick_rockets",
      1,
      0,
      0
     ],
     [
      "unload_entities",
      1,
      0,
      0
     ],
     [
      "touch_arrows",
      1,
      0,
      0
     ],
     [
      "tick_thunder",
      1,
      0,
      0
     ],
     [
      "tick_paintings",
      1,
      0,
      0
     ],
     [
      "tick_using",
      1,
      0,
      0
     ],
     [
      "tick_bolts",
      1,
      0,
      0
     ],
     [
      "tick_lecterns",
      1,
      0,
      0
     ],
     [
      "tick_tridents",
      1,
      0,
      0
     ]
    ]
   }
  ],
  "dimensions": [
   {
    "id": "d:minecraft:overworld",
    "name": "minecraft:overworld",
    "chunks": 841,
    "entities": 372,
    "block_entities": 97
   }
  ],
  "entities": [
   {
    "by": "entity",
    "key": null,
    "label": "(none)",
    "value": 2.84,
    "pct": 50.644,
    "samples": 629
   },
   {
    "by": "entity",
    "key": "e:minecraft:cow",
    "label": "minecraft:cow",
    "value": 0.343,
    "pct": 6.119,
    "samples": 76
   },
   {
    "by": "entity",
    "key": "e:minecraft:skeleton",
    "label": "minecraft:skeleton",
    "value": 0.312,
    "pct": 5.556,
    "samples": 69
   },
   {
    "by": "entity",
    "key": "e:minecraft:sheep",
    "label": "minecraft:sheep",
    "value": 0.298,
    "pct": 5.314,
    "samples": 66
   },
   {
    "by": "entity",
    "key": "e:minecraft:creeper",
    "label": "minecraft:creeper",
    "value": 0.253,
    "pct": 4.509,
    "samples": 56
   },
   {
    "by": "entity",
    "key": "e:minecraft:pig",
    "label": "minecraft:pig",
    "value": 0.248,
    "pct": 4.428,
    "samples": 55
   }
  ],
  "blocks": [
   {
    "by": "block",
    "key": null,
    "label": "(none)",
    "value": 4.736,
    "pct": 84.461,
    "samples": 1049
   },
   {
    "by": "block",
    "key": "b:minecraft:grass_block",
    "label": "minecraft:grass_block",
    "value": 0.781,
    "pct": 13.929,
    "samples": 173
   },
   {
    "by": "block",
    "key": "b:minecraft:lava",
    "label": "minecraft:lava",
    "value": 0.086,
    "pct": 1.53,
    "samples": 19
   },
   {
    "by": "block",
    "key": "b:minecraft:sugar_cane",
    "label": "minecraft:sugar_cane",
    "value": 0.005,
    "pct": 0.081,
    "samples": 1
   }
  ],
  "costs": [
   {
    "id": "e:minecraft:sheep",
    "name": "minecraft:sheep",
    "dimension": "d:minecraft:overworld",
    "ms_per_tick": 0.307,
    "mobs_ticked": 30.86,
    "us_per_mob": 9.956
   },
   {
    "id": "e:minecraft:cow",
    "name": "minecraft:cow",
    "dimension": "d:minecraft:overworld",
    "ms_per_tick": 0.266,
    "mobs_ticked": 28.842,
    "us_per_mob": 9.209
   },
   {
    "id": "e:minecraft:zombie",
    "name": "minecraft:zombie",
    "dimension": "d:minecraft:overworld",
    "ms_per_tick": 0.239,
    "mobs_ticked": 23.459,
    "us_per_mob": 10.169
   },
   {
    "id": "e:minecraft:pig",
    "name": "minecraft:pig",
    "dimension": "d:minecraft:overworld",
    "ms_per_tick": 0.236,
    "mobs_ticked": 25.883,
    "us_per_mob": 9.126
   },
   {
    "id": "e:minecraft:skeleton",
    "name": "minecraft:skeleton",
    "dimension": "d:minecraft:overworld",
    "ms_per_tick": 0.222,
    "mobs_ticked": 21.396,
    "us_per_mob": 10.372
   },
   {
    "id": "e:minecraft:creeper",
    "name": "minecraft:creeper",
    "dimension": "d:minecraft:overworld",
    "ms_per_tick": 0.201,
    "mobs_ticked": 23.113,
    "us_per_mob": 8.71
   }
  ],
  "net": {
   "ok": true,
   "op": "net",
   "captures": [
    {
     "file": "profile-20261108-132243Z.hzperf",
     "sides": [
      "server",
      "client"
     ],
     "kind": "integrated",
     "seconds": 44.398
    }
   ],
   "selection": {
    "side": "server",
    "thread": "Server thread",
    "select": "cpu",
    "slow": false,
    "where": {},
    "from": 0,
    "to": 44.398,
    "ticks": 886,
    "samples": null
   },
   "unit": "kib_per_s",
   "rows": [
    {
     "id": "n:all",
     "players": 1,
     "out_bytes": 22704080,
     "in_bytes": 3251,
     "out_kib_s": 499.394,
     "in_kib_s": 0.072
    },
    {
     "id": "n:p1",
     "index": 1,
     "joined_t": 1.086,
     "left_t": null,
     "out_bytes": 22704080,
     "in_bytes": 3251,
     "packets_out": 0,
     "packets_in": 0,
     "out_kib_s": 511.922,
     "in_kib_s": 0.073,
     "peak_out_kib_s": 4392.712
    }
   ],
   "warnings": []
  }
 },
 "text": "profile-20261108-132243Z.hzperf: integrated server, 44.4 s, Horizon 0.1.0 release, Linux x86_64, 14 threads\nhealth  TPS 20.0/20 · MSPT avg 5.7 · worst-second p95 19.8 · max 53.8 ms · 2 of 886 ticks over 50 ms\n        CPU 0.6 cores (system 4.7 of 14) · RSS 917 MB (-53 MB over the window) · 1 players · 841 chunks · 295 entities\ntick phases                    ms/tick   %MSPT  cpu ms  waited\n  level_overworld                  5.8    98.6     5.8     0.0   [s:level_overworld]\n    tick_mobs                      3.1    52.2     3.1     0.0   [s:tick_mobs]\n      tick_creature_entities       2.6    44.4     2.6     0.0   [s:tick_creature_entities]\n        creature_loop              2.3    39.6     2.3     0.0   [s:creature_loop]\n        creature_prepare           0.2     3.5     0.2     0.0   [s:creature_prepare]\n      spawn_natural                0.4     6.7     0.4     0.0   [s:spawn_natural]\n        sn_attempts                0.3     5.2     0.3     0.0   [s:sn_attempts]\n    tick_blocks                    0.9    14.8     0.9     0.0   [s:tick_blocks]\n      random_ticks                 0.6     9.5     0.6     0.0   [s:random_ticks]\n      tb_entity_inside             0.2     4.0     0.2     0.0   [s:tb_entity_inside]\n    tick_lan                       0.8    12.8     0.8     0.0   [s:tick_lan]\n      lan_entity_states            0.4     6.1     0.4     0.0   [s:lan_entity_states]\n      lan_tick_level               0.2     2.7     0.2     0.0   [s:lan_tick_level]\n        lan_track_entities         0.1     1.7     0.1     0.0   [s:lan_track_entities]\n        lan_chunks                 0.1     0.9     0.1     0.0   [s:lan_chunks]\n      lan_guests                   0.1     1.9     0.1     0.0   [s:lan_guests]\n    tick_items                     0.3     4.9     0.3     0.0   [s:tick_items]\n    tick_carts                     0.2     2.7     0.2     0.0   [s:tick_carts]\n    tick_sculk_entities            0.1     2.2     0.1     0.0   [s:tick_sculk_entities]\n      sculk_entity_events          0.1     2.0     0.1     0.0   [s:sculk_entity_events]\nhot functions, tick thread, CPU, all ticks    self ms/tick total ms/tick\n  PalettedContainer::get                              0.58          0.58   [f:kq3h1qx3]\n  RandomTickSections::next_maske                      0.20          0.28   [f:prmwv9hs]\n  ai::tick                                            0.20          2.12   [f:kqnrkqat]\n  __memmove_avx_unaligned_erms                        0.19          0.19   [f:k61z3gs5]\n  _mi_page_malloc_zero                                0.19          0.19   [f:ne9h54ke]\n  Behaviours::tick_chunk_random_                      0.19          0.61   [f:vv8h8aba]\n  World::block                                        0.18          0.19   [f:kd5efx0c]\n  ptr::drop_glue                                      0.18          0.22   [f:q0p6pgdz]\nby entity type (sampled ms/tick): minecraft:cow 0.3 [e:minecraft:cow] · minecraft:skeleton 0.3 [e:minecraft:skeleton] · minecraft:sheep 0.3 [e:minecraft:sheep] · minecraft:creeper 0.3 [e:minecraft:creeper]\nby block type (sampled ms/tick): minecraft:grass_block 0.8 [b:minecraft:grass_block] · minecraft:lava 0.1 [b:minecraft:lava] · minecraft:sugar_cane 0.0 [b:minecraft:sugar_cane]\nby chunk (sampled ms/tick): overworld/-1,-6 0.0 [c:overworld/-1,-6] · overworld/-14,-7 0.0 [c:overworld/-14,-7] · overworld/-6,-7 0.0 [c:overworld/-6,-7]\nworst   96.8 ms at 0.2 s (tick_lan_packets 9.3, tick_hud 7.9) [k:1]\n        53.8 ms at 15.1 s (tick_mobs 3.5, tick_lan 1.2) [k:300]\n        27.8 ms at 44.1 s (tick_mobs 17.0, tick_lan 6.4) [k:880]\n        22.3 ms at 0.3 s (tick_blocks 11.5, tick_mobs 4.7) [k:4]\n        19.8 ms at 44.0 s (tick_mobs 16.2, tick_blocks 1.1) [k:878]\nnet     out 499 KiB/s, in 0 KiB/s · per player out p50 512 KiB/s p95 512 KiB/s max 512 KiB/s [n:p1 max]\nworld   minecraft:overworld 841 chunks 372 entities [d:minecraft:overworld]\n        by entity type (sampled ms/tick): minecraft:sheep 0.3 [e:minecraft:sheep] · minecraft:cow 0.3 [e:minecraft:cow] · minecraft:zombie 0.2 [e:minecraft:zombie] · minecraft:pig 0.2 [e:minecraft:pig]\nworkers generate 0.06 cores · mesh 0.03 · load 0.01 · other 0.00 (server)\ncost    spans 0.01 ms/tick (0.15% of the tick thread) · sampler 0.01% of the tick thread · writer 0.10% of a core · 0 samples dropped\nnote    chunk coordinates and entity/block type names are included; no names, UUIDs, addresses, seed or world name\nnext    horizon-perf query ticks --worst 5 --side server · horizon-perf query series h:mspt_p95 s:level_overworld --from -900 --side server · horizon-perf query callers f:kq3h1qx3 --side server\n"
};
