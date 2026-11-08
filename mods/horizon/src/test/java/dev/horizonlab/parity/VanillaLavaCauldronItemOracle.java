package dev.horizonlab.parity;

import java.lang.reflect.Field;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import net.minecraft.core.BlockPos;
import net.minecraft.core.RegistryAccess;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.damagesource.DamageSources;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.InsideBlockEffectApplier;
import net.minecraft.world.entity.item.ItemEntity;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.phys.Vec3;

/** Actual cauldron inside shape, block effects and ItemEntity damage, on a
 * constructor-free stand-in level. No server or world is started. */
public final class VanillaLavaCauldronItemOracle {
    static final class Lvl extends ServerLevel {
        int next;
        DamageSources damage;
        net.minecraft.world.level.gamerules.GameRules rules;
        Lvl() { super(null, null, null, null, null, null, false, 0L, List.of(), false); }
        @Override public int getNextEntityId() { return next++; }
        @Override public RegistryAccess registryAccess() { return ParityData.access; }
        @Override public DamageSources damageSources() { return damage; }
        @Override public net.minecraft.world.level.gamerules.GameRules getGameRules() { return rules; }
        @Override public void gameEvent(net.minecraft.core.Holder<net.minecraft.world.level.gameevent.GameEvent> e, Vec3 p, net.minecraft.world.level.gameevent.GameEvent.Context c) {}
        @Override public void playSound(Entity e, double x, double y, double z, net.minecraft.sounds.SoundEvent s, net.minecraft.sounds.SoundSource c, float v, float pitch) {}
    }

    public static void main(String[] args) throws Exception {
        ParityData.init();
        var unsafeField = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        unsafeField.setAccessible(true);
        var unsafe = unsafeField.get(null);
        var allocate = unsafe.getClass().getMethod("allocateInstance", Class.class);
        var level = (Lvl) allocate.invoke(unsafe, Lvl.class);
        Field registry = Level.class.getDeclaredField("registryAccess");
        long offset = (long) unsafe.getClass().getMethod("objectFieldOffset", Field.class).invoke(unsafe, registry);
        unsafe.getClass().getMethod("putObject", Object.class, long.class, Object.class).invoke(unsafe, level, offset, ParityData.access);
        level.damage = new DamageSources(ParityData.access);
        level.rules = new net.minecraft.world.level.gamerules.GameRules(net.minecraft.world.flag.FeatureFlags.REGISTRY.allFlags());
        Field health = ItemEntity.class.getDeclaredField("health");
        health.setAccessible(true);
        List<String> output = new ArrayList<>();
        for (String line : Files.readAllLines(Path.of(args[0]))) {
            String[] f = line.split("\t");
            var item = new ItemEntity(level, Double.parseDouble(f[2]), Double.parseDouble(f[3]), Double.parseDouble(f[4]), ParityData.stack(f[1]));
            var state = (f[5].equals("lava") ? Blocks.LAVA_CAULDRON : Blocks.CAULDRON).defaultBlockState();
            var shape = state.getEntityInsideCollisionShape(level, BlockPos.ZERO, item);
            boolean inside = shape.toAabbs().stream().anyMatch(b -> b.intersects(item.getBoundingBox().deflate(1.0e-5)));
            List<String> steps = new ArrayList<>();
            for (int tick = 0; tick < 2; tick++) {
                // The preceding baseTick fire phase, without ticking a level.
                // The fixture contains cauldrons, not ordinary lava fluid.
                int fire = item.getRemainingFireTicks();
                if (fire > 0) {
                    if (fire % 20 == 0) item.hurtServer(level, level.damage.onFire(), 1.0f);
                    item.setRemainingFireTicks(fire - 1);
                }
                if (inside && !item.isRemoved()) {
                    var effects = new InsideBlockEffectApplier.StepBasedCollector();
                    effects.advanceStep(0);
                    state.entityInside(level, BlockPos.ZERO, item, effects, true);
                    effects.applyAndClear(item);
                }
                steps.add(health.getInt(item) + "," + item.getRemainingFireTicks() + "," + item.isRemoved());
            }
            output.add(f[0] + "\t" + String.join("\t", steps));
        }
        Files.write(Path.of(args[1]), output);
    }
}
