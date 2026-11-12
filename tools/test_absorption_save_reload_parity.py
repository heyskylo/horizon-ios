#!/usr/bin/env python3
"""A loaded mob's AbsorptionAmount is kept as saved, negative or above the maximum.

`LivingEntity.readAdditionalSaveData` stores it with `internalSetAbsorptionAmount`, which does not
clamp (only `setAbsorptionAmount` does), and `addAdditionalSaveData` writes it back as it is. Pigs
are summoned with several amounts, the world is saved and reloaded (tools/entity_save_reload.py),
and each pig's `AbsorptionAmount` is read with `/data get` on vanilla's server and Horizon's.
"""
try:
    from . import entity_save_reload as harness
except ImportError:
    import entity_save_reload as harness

# (tag, saved AbsorptionAmount or None for the field absent)
CASES = [('absent', None), ('zero', '0.0f'), ('negative', '-1.0f'), ('small', '3.5f'), ('huge', '1.0E30f')]
KEEP = 'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b'

SETUP = [f'summon minecraft:pig {2 + 2 * i}.5 -60 2.5 {{Tags:["{tag}"],{KEEP}'
         + (f',AbsorptionAmount:{amount}' if amount else '') + '}'
         for i, (tag, amount) in enumerate(CASES)]
PROBES = [(tag, f'data get entity @e[tag={tag},limit=1] AbsorptionAmount') for tag, _ in CASES]


def main():
    return harness.main('absorption', SETUP, PROBES, __doc__)


if __name__ == '__main__':
    raise SystemExit(main())
