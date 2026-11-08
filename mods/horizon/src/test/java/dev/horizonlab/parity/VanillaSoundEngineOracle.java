package dev.horizonlab.parity;

import java.io.PrintWriter;
import java.lang.reflect.Constructor;
import java.nio.file.*;
import java.util.*;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.client.player.LocalPlayer;
import net.minecraft.client.resources.sounds.*;
import net.minecraft.resources.Identifier;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundEvents;
import net.minecraft.sounds.SoundSource;
import net.minecraft.util.RandomSource;
import net.minecraft.world.TickRateManager;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.vehicle.minecart.Minecart;
import net.minecraft.world.level.Level;
import net.minecraft.world.phys.Vec3;

/**
 * Drives the real SoundEngine (see VanillaSoundRig) with a script of plays,
 * stops, category gains, option volumes, channel ends, pauses and ticks, and
 * prints after every step each instance's activity and AL source parameters,
 * the subtitle notifications and the channel pool counts.
 */
public final class VanillaSoundEngineOracle {
    /** The local player the underwater instances look at. */
    public static final class Actor extends LocalPlayer {
        boolean underwater;

        Actor() {
            super(null, null, null, null, null, null, false, null, null);
        }

        @Override
        public boolean isUnderWater() {
            return underwater;
        }
    }

    /** A minecart the minecart sound instances follow. */
    public static final class Cart extends Minecart {
        boolean silent;
        Cart() {
            super(null, null);
        }

        @Override
        public boolean isSilent() {
            return silent;
        }
    }

    static float f(String hex) {
        return Float.intBitsToFloat(Integer.parseUnsignedInt(hex, 16));
    }

    static double d(String hex) {
        return Double.longBitsToDouble(Long.parseUnsignedLong(hex, 16));
    }

    static void seed(SoundInstance instance, long seed) throws Exception {
        VanillaSoundRig.set(instance, AbstractSoundInstance.class, "random", RandomSource.create(seed));
    }

    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        var rig = new VanillaSoundRig(Path.of(args[0]));
        var actor = VanillaSoundRig.make(Actor.class);
        Constructor<UnderLiquidSubSound> sub = UnderLiquidSubSound.class.getDeclaredConstructor(LocalPlayer.class, SoundEvent.class);
        sub.setAccessible(true);
        var instances = new TreeMap<String, SoundInstance>();
        var carts = new TreeMap<String, Cart>();
        var level = VanillaSoundRig.make(ClientLevel.class);
        var tickRate = new TickRateManager();
        VanillaSoundRig.set(level, ClientLevel.class, "tickRateManager", tickRate);
        var manager = rig.manager;
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[2])))) {
            for (String line : Files.readAllLines(Path.of(args[1]))) {
                out.println(line);
                String[] a = line.split(" ");
                switch (a[0]) {
                    case "play" -> {
                        // play name kind event source volume pitch x y z looping delay attenuation relative seed
                        Identifier id = Identifier.parse(a[3]);
                        SoundEvent event = SoundEvent.createVariableRangeEvent(id);
                        long seed = Long.parseLong(a[14]);
                        SoundInstance instance = switch (a[2]) {
                            case "simple" -> new SimpleSoundInstance(id, SoundSource.values()[Integer.parseInt(a[4])], f(a[5]), f(a[6]),
                                RandomSource.create(seed), a[10].equals("1"), Integer.parseInt(a[11]),
                                a[12].equals("linear") ? SoundInstance.Attenuation.LINEAR : SoundInstance.Attenuation.NONE, d(a[7]), d(a[8]), d(a[9]),
                                a[13].equals("1"));
                            case "loop" -> new BiomeAmbientSoundsHandler.LoopSoundInstance(event);
                            case "uloop" -> new UnderLiquidAmbientSoundInstance(event, actor, LocalPlayer::isUnderWater);
                            case "usub" -> sub.newInstance(actor, event);
                            case "bound" -> new EntityBoundSoundInstance(event,SoundSource.values()[Integer.parseInt(a[4])],f(a[5]),f(a[6]),carts.get(a[15]),seed);
                            case "cart" -> new MinecartSoundInstance(carts.get(a[3]));
                            case "rcart", "rcartu" -> new RidingMinecartSoundInstance(actor, carts.get(a[3]), a[2].equals("rcartu"),
                                a[2].equals("rcartu") ? SoundEvents.MINECART_INSIDE_UNDERWATER : SoundEvents.MINECART_INSIDE, 0.0F, 0.75F, 1.0F);
                            default -> throw new IllegalArgumentException(line);
                        };
                        if (!a[2].equals("simple")) seed(instance, seed);
                        instances.put(a[1], instance);
                        out.println("  result " + manager.play(instance));
                    }
                    case "replay" -> out.println("  result " + manager.play(instances.get(a[1])));
                    case "queue" -> manager.queueTickingSound((TickableSoundInstance) instances.get(a[1]));
                    case "delayed" -> manager.playDelayed(instances.get(a[1]), Integer.parseInt(a[2]));
                    case "fade" -> {
                        if (instances.get(a[1]) instanceof BiomeAmbientSoundsHandler.LoopSoundInstance loop) {
                            if (a[2].equals("in")) loop.fadeIn(); else loop.fadeOut();
                        }
                    }
                    case "stop" -> manager.stop(instances.get(a[1]));
                    case "stopmatch" -> manager.stop(a[1].equals("-") ? null : Identifier.parse(a[1]),
                        a[2].equals("-") ? null : SoundSource.values()[Integer.parseInt(a[2])]);
                    case "finish" -> rig.finish(instances.get(a[1]));
                    case "gain" -> manager.updateCategoryVolume(SoundSource.values()[Integer.parseInt(a[1])], f(a[2]));
                    case "volume" -> rig.volume(SoundSource.values()[Integer.parseInt(a[1])], f(a[2]));
                    case "refresh" -> manager.refreshCategoryVolume(SoundSource.values()[Integer.parseInt(a[1])]);
                    case "pause" -> manager.pauseAllExcept(SoundSource.MUSIC, SoundSource.UI);
                    case "resume" -> manager.resume();
                    case "player" -> {
                        actor.underwater = a[1].equals("1");
                        VanillaSoundRig.set(actor, Entity.class, "removalReason", a[2].equals("1") ? Entity.RemovalReason.DISCARDED : null);
                    }
                    case "cart", "cartnew" -> {
                        // cart name x y z dx dy dz removed
                        Cart cart = carts.get(a[1]);
                        if (cart == null || a[0].equals("cartnew")) {
                            if(cart != null)VanillaSoundRig.set(cart,Entity.class,"removalReason",Entity.RemovalReason.DISCARDED);
                            cart = VanillaSoundRig.make(Cart.class);
                            VanillaSoundRig.set(cart, Entity.class, "level", level);
                            carts.put(a[1], cart);
                        }
                        cart.silent=a.length>9 && a[9].equals("1");
                        VanillaSoundRig.set(cart, Entity.class, "position", new Vec3(d(a[2]), d(a[3]), d(a[4])));
                        VanillaSoundRig.set(cart, Entity.class, "deltaMovement", new Vec3(d(a[5]), d(a[6]), d(a[7])));
                        VanillaSoundRig.set(cart, Entity.class, "removalReason", a[8].equals("1") ? Entity.RemovalReason.DISCARDED : null);
                    }
                    case "ride" -> VanillaSoundRig.set(actor, Entity.class, "vehicle", a[1].equals("-") ? null : carts.get(a[1]));
                    case "freeze" -> {
                        tickRate.setFrozen(a[1].equals("1"));
                        tickRate.tick();
                    }
                    case "tick" -> manager.tick(a[1].equals("1"));
                    case "forget" -> instances.remove(a[1]);
                    default -> throw new IllegalArgumentException(line);
                }
                for (var e : instances.entrySet()) out.println(rig.describe(e.getKey(), e.getValue()));
                rig.flush(out);
            }
        }
        System.exit(0);
    }
}
