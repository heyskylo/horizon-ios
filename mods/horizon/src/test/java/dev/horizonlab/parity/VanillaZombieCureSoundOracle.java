package dev.horizonlab.parity;

import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundSource;
import net.minecraft.util.RandomSource;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.monster.zombie.ZombieVillager;
import net.minecraft.world.phys.Vec3;

/** Records the real ZombieVillager.handleEntityEvent(16), without an audio engine. */
public final class VanillaZombieCureSoundOracle {
    static Field field(Class<?> owner, String name) throws Exception {
        var f = owner.getDeclaredField(name); f.setAccessible(true); return f;
    }
    static String d(double value) { return String.format("%016x", Double.doubleToRawLongBits(value)); }
    static final class World extends ClientLevel {
        String sound;
        World() { super(null, null, null, null, 0, 0, null, false, 0L, 0); }
        @Override public void playLocalSound(double x, double y, double z, SoundEvent event,
                SoundSource source, float volume, float pitch, boolean delay) {
            if (sound != null) throw new AssertionError("More than one cure sound");
            if (delay) throw new AssertionError("Unexpected delayed cure sound");
            sound = BuiltInRegistries.SOUND_EVENT.getKey(event) + " " + d(x) + " " + d(y) + " " + d(z)
                + " " + String.format("%08x %08x", Float.floatToRawIntBits(volume), Float.floatToRawIntBits(pitch));
        }
    }
    static final class Actor extends ZombieVillager {
        boolean silent;
        Actor() { super(null, null); }
        @Override public boolean isSilent() { return silent; }
        @Override public SoundSource getSoundSource() { return SoundSource.HOSTILE; }
    }
    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion(); net.minecraft.server.Bootstrap.bootStrap();
        var u = Class.forName("sun.misc.Unsafe"); var unsafe = field(u, "theUnsafe").get(null);
        var allocate = u.getMethod("allocateInstance", Class.class);
        var world = (World) allocate.invoke(unsafe, World.class);
        var actor = (Actor) allocate.invoke(unsafe, Actor.class);
        field(Entity.class, "level").set(actor, world);
        var position = field(Entity.class, "position");
        var eye = field(Entity.class, "eyeHeight");
        var random = field(Entity.class, "random");
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1])))) {
            for (String line : Files.readAllLines(Path.of(args[0]))) {
                var p = line.split(" ");
                var rng = RandomSource.create(Long.parseLong(p[0]));
                actor.silent = p[1].equals("1");
                position.set(actor, new Vec3(Double.parseDouble(p[2]), Double.parseDouble(p[3]), Double.parseDouble(p[4])));
                eye.setFloat(actor, Float.parseFloat(p[5]));
                random.set(actor, rng);
                world.sound = null;
                actor.handleEntityEvent((byte) 16);
                out.println((world.sound == null ? "silent" : world.sound) + " next " + rng.nextLong());
            }
        }
    }
}
