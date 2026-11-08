package dev.horizonlab.parity;

import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.nio.file.Path;
import java.util.*;
import java.util.function.Predicate;
import net.minecraft.core.*;
import net.minecraft.network.protocol.Packet;
import net.minecraft.network.protocol.game.ClientboundExplodePacket;
import net.minecraft.server.level.*;
import net.minecraft.server.network.ServerGamePacketListenerImpl;
import net.minecraft.util.RandomSource;
import net.minecraft.world.damagesource.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.projectile.throwableitemprojectile.AbstractThrownPotion;
import net.minecraft.world.entity.projectile.throwableitemprojectile.ThrownExperienceBottle;
import net.minecraft.world.level.entity.EntityTypeTest;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.*;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.level.gameevent.GameEvent;
import net.minecraft.world.phys.*;

/** Real ServerLevel.explode and AbstractThrownPotion.onHit on isolated recording worlds.
 * Radius zero avoids terrain and damage; potion effects are counted at onHitAsPotion's seam.
 * Bottle XP and next-world-RNG silence invariance use the real onHit/orb-award methods.
 * No server, window or audio device is started.
 */
public final class VanillaSourceSilenceOracle {
    static Object unsafe;
    static <T> T allocate(Class<T> type) throws Exception {
        return type.cast(unsafe.getClass().getMethod("allocateInstance", Class.class).invoke(unsafe, type));
    }
    static void put(Object target, Class<?> owner, String field, Object value) throws Exception {
        Field f = owner.getDeclaredField(field);
        long offset = (long) unsafe.getClass().getMethod("objectFieldOffset", Field.class).invoke(unsafe, f);
        unsafe.getClass().getMethod("putObject", Object.class, long.class, Object.class).invoke(unsafe, target, offset, value);
    }
    static final class World extends ServerLevel {
        List<Integer> events;
        GameRules rules;
        List<Orb> orbs;
        World() { super(null, null, null, null, null, null, false, 0L, List.of(), false); }
        @Override public GameRules getGameRules() { return rules; }
        @Override public void gameEvent(Entity source, Holder<GameEvent> event, Vec3 at) {}
        @Override public void levelEvent(Entity source, int event, BlockPos at, int data) { events.add(event); }
        @Override public <T extends Entity> List<T> getEntitiesOfClass(Class<T> type, AABB box, Predicate<? super T> predicate) {
            return List.of();
        }
        @Override public <T extends Entity> List<T> getEntities(EntityTypeTest<Entity, T> type, AABB box, Predicate<? super T> predicate) {
            for (Orb orb : orbs) {
                T candidate = type.tryCast(orb);
                if (candidate != null && predicate.test(candidate)) return List.of(candidate);
            }
            throw new AssertionError("missing preexisting orb merge target");
        }
    }
    static final class Connection extends ServerGamePacketListenerImpl {
        ClientboundExplodePacket explosion;
        Connection() { super(null, null, null, null); }
        @Override public void send(Packet<?> packet) { explosion = (ClientboundExplodePacket) packet; }
    }
    static final class Player extends ServerPlayer {
        Player() { super(null, null, null, null); }
        @Override public double distanceToSqr(Vec3 at) { return 0.0; }
    }
    static final class Potion extends AbstractThrownPotion {
        ItemStack stack;
        boolean silent;
        int effects;
        boolean removed;
        Potion() { super(EntityTypes.SPLASH_POTION, null); }
        @Override public ItemStack getItem() { return stack; }
        @Override protected Item getDefaultItem() { return Items.SPLASH_POTION; }
        @Override public boolean isSilent() { return silent; }
        @Override public BlockPos blockPosition() { return BlockPos.ZERO; }
        @Override public void remove(Entity.RemovalReason reason) { removed = true; }
        @Override protected void onHitAsPotion(ServerLevel world, ItemStack stack, HitResult hit) { effects++; }
        void impact() { onHit(BlockHitResult.miss(Vec3.ZERO, Direction.UP, BlockPos.ZERO)); }
    }
    static final class Bottle extends ThrownExperienceBottle {
        boolean silent;
        Bottle() { super(EntityTypes.EXPERIENCE_BOTTLE, null); }
        @Override public boolean isSilent() { return silent; }
        @Override public BlockPos blockPosition() { return BlockPos.ZERO; }
        @Override public void remove(Entity.RemovalReason reason) {}
        void impact() { onHit(BlockHitResult.miss(Vec3.ZERO, Direction.UP, BlockPos.ZERO)); }
    }
    static final class Orb extends ExperienceOrb {
        int value;
        Orb() { super(EntityTypes.EXPERIENCE_ORB, null); }
        @Override public int getValue() { return value; }
    }
    public static void main(String[] args) throws Exception {
        ParityData.init();
        Field f = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        f.setAccessible(true);
        unsafe = f.get(null);
        World world = allocate(World.class);
        world.events = new ArrayList<>();
        world.rules = new GameRules(FeatureFlags.DEFAULT_FLAGS);
        put(world, Level.class, "random", RandomSource.create(42));
        Player player = allocate(Player.class);
        Connection connection = allocate(Connection.class);
        player.connection = connection;
        put(world, ServerLevel.class, "players", List.of(player));
        Potion source = allocate(Potion.class);
        put(source, Entity.class, "level", world);
        source.setBoundingBox(new AABB(0, 0, 0, .25, .25, .25));
        var damage = new DamageSource(ParityData.access.lookupOrThrow(net.minecraft.core.registries.Registries.DAMAGE_TYPE)
            .getOrThrow(DamageTypes.EXPLOSION));
        try (var out = new PrintWriter(Path.of(args[0]).toFile())) {
            for (boolean silent : new boolean[]{false, true, false}) {
                source.silent = silent;
                world.explode(source, damage, null, 0, 0, 0, 0, false, Level.ExplosionInteraction.NONE);
                out.println("explosion " + silent + " " + (connection.explosion.explosionParticle() != null)
                    + " " + connection.explosion.playSound());
            }
            world.explode(null, damage, null, 0, 0, 0, 0, false, Level.ExplosionInteraction.NONE);
            if (!connection.explosion.playSound()) throw new AssertionError("source-free blast lost its sound");
            for (String kind : List.of("splash_potion", "lingering_potion")) {
                for (String potion : List.of("awkward", "swiftness", "healing")) {
                    source.stack = ParityData.stack("{id:'minecraft:" + kind
                        + "',count:1,components:{'minecraft:potion_contents':{potion:'minecraft:" + potion + "'}}}");
                    for (boolean silent : new boolean[]{false, true}) {
                        source.silent = silent;
                        source.effects = 0;
                        source.removed = false;
                        world.events.clear();
                        source.impact();
                        if (source.effects != (potion.equals("awkward") ? 0 : 1) || !source.removed)
                            throw new AssertionError("silence changed potion effects/removal");
                        out.println(kind + " " + potion + " " + silent + " "
                            + (world.events.contains(2002) || world.events.contains(2007)) + " "
                            + (world.events.contains(1053) || world.events.contains(1054)));
                    }
                }
            }
            // Exercise the real bottle and orb-award methods, merging into recording
            // targets so unrelated entity constructors need no live world. Compare
            // silence invariance, not absolute entity/world RNG ownership parity.
            world.orbs = new ArrayList<>();
            Field count = ExperienceOrb.class.getDeclaredField("count");
            count.setAccessible(true);
            for (int value : new int[]{1, 3, 7}) {
                for (int slot = 0; slot < 40; slot++) {
                    Orb orb = allocate(Orb.class);
                    orb.value = value;
                    orb.setId(slot + 40);
                    world.orbs.add(orb);
                }
            }
            Bottle bottle = allocate(Bottle.class);
            put(bottle, Entity.class, "level", world);
            for (long seed : new long[]{0, 1, -1, 42, Long.MIN_VALUE, Long.MAX_VALUE}) {
                int audibleXp = 0;
                long audibleNext = 0;
                for (boolean silent : new boolean[]{false, true}) {
                    for (Orb orb : world.orbs) count.setInt(orb, 0);
                    bottle.silent = silent;
                    put(bottle, Entity.class, "random", RandomSource.create(seed));
                    put(world, Level.class, "random", RandomSource.create(seed ^ 0x5850574f524c44L));
                    bottle.impact();
                    int xp = 0;
                    for (Orb orb : world.orbs) xp += orb.value * count.getInt(orb);
                    long next = world.getRandom().nextLong();
                    if (xp < 3 || xp > 11) throw new AssertionError("bottle lost XP");
                    if (!silent) { audibleXp = xp; audibleNext = next; }
                    else out.println("bottle_rng " + seed + " " + (xp == audibleXp) + " " + (next == audibleNext));
                }
            }
        }
    }
}
