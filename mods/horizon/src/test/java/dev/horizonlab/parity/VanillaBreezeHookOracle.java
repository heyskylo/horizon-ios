package dev.horizonlab.parity;

import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.core.Holder;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundEvents;
import net.minecraft.sounds.SoundSource;
import net.minecraft.util.RandomSource;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.monster.breeze.Breeze;
import net.minecraft.world.entity.projectile.FishingHook;
import net.minecraft.world.entity.projectile.Projectile;
import net.minecraft.world.level.gameevent.GameEvent;
import net.minecraft.world.phys.EntityHitResult;
import net.minecraft.world.phys.HitResult;
import net.minecraft.world.phys.Vec3;

/** Original recording oracle: native fishing-hook collisions with default-tagged Breezes. */
public final class VanillaBreezeHookOracle {
    static Field field(Class<?> owner, String name) throws Exception {
        var field = owner.getDeclaredField(name);
        field.setAccessible(true);
        return field;
    }
    static String bits(double value) { return String.format("%016x", Double.doubleToRawLongBits(value)); }
    static final class World extends ClientLevel {
        int sounds, lands;
        World() { super(null, null, null, null, 0, 0, null, false, 0L, 0); }
        @Override public boolean isClientSide() { return false; }
        @Override public void playSound(Entity except, Entity source, SoundEvent sound,
                SoundSource category, float volume, float pitch) {
            if (sound != SoundEvents.BREEZE_DEFLECT || volume != 1.0F || pitch != 1.0F) {
                throw new AssertionError("Unexpected deflection sound");
            }
            sounds++;
        }
        @Override public void gameEvent(Holder<GameEvent> event, Vec3 pos, GameEvent.Context context) {
            if (event.equals(GameEvent.PROJECTILE_LAND)) lands++;
            else throw new AssertionError("Unexpected game event");
        }
    }
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var unsafeClass = Class.forName("sun.misc.Unsafe");
        var unsafe = field(unsafeClass, "theUnsafe").get(null);
        var allocate = unsafeClass.getMethod("allocateInstance", Class.class);
        var level = field(Entity.class, "level");
        var type = field(Entity.class, "type");
        var position = field(Entity.class, "position");
        var random = field(Entity.class, "random");
        var collision = Projectile.class.getDeclaredMethod("hitTargetOrDeflectSelf", HitResult.class);
        collision.setAccessible(true);
        var attached = field(FishingHook.class, "hookedIn");
        var world = (World) allocate.invoke(unsafe, World.class);
        var breezes = new Breeze[2];
        for (int i = 0; i < breezes.length; i++) {
            breezes[i] = (Breeze) allocate.invoke(unsafe, Breeze.class);
            type.set(breezes[i], EntityTypes.BREEZE);
            level.set(breezes[i], world);
            position.set(breezes[i], new Vec3(i, 64, 0));
        }
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8))) {
            for (String line : Files.readAllLines(Path.of(args[0]))) {
                var parts = line.split(" ");
                var hook = (FishingHook) allocate.invoke(unsafe, FishingHook.class);
                type.set(hook, EntityTypes.FISHING_BOBBER);
                level.set(hook, world);
                position.set(hook, Vec3.ZERO);
                random.set(hook, RandomSource.create(Long.parseLong(parts[0])));
                hook.setDeltaMovement(new Vec3(Double.parseDouble(parts[1]), Double.parseDouble(parts[2]), Double.parseDouble(parts[3])));
                var trace = new ArrayList<String>();
                for (int i = 4; i < parts.length; i++) {
                    hook.needsSync = false;
                    world.sounds = world.lands = 0;
                    collision.invoke(hook, new EntityHitResult(breezes[Integer.parseInt(parts[i])]));
                    var velocity = hook.getDeltaMovement();
                    trace.add(bits(velocity.x) + "," + bits(velocity.y) + "," + bits(velocity.z)
                        + ":" + world.sounds + ":" + world.lands + ":" + (hook.needsSync ? 1 : 0)
                        + ":" + (attached.get(hook) == null ? 0 : 1));
                }
                out.println(String.join(" ", trace));
            }
        }
    }
}
