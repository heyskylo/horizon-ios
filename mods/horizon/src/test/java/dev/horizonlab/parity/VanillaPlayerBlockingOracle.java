package dev.horizonlab.parity;

import com.google.gson.JsonParser;
import java.io.PrintWriter;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.List;
import net.minecraft.server.PlayerAdvancements;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.stats.Stat;
import net.minecraft.stats.Stats;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.damagesource.DamageSource;
import net.minecraft.world.entity.EquipmentSlot;
import net.minecraft.world.entity.LivingEntity;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.phys.Vec3;

/** Native applyItemBlocking, including its actual stat and ItemStack wear dispatch. */
public final class VanillaPlayerBlockingOracle {
    static Object unsafe;
    static Method allocate;
    static <T> T make(Class<T> type) throws Exception {
        return type.cast(allocate.invoke(unsafe, type));
    }
    static final class Level extends ServerLevel {
        Level() { super(null, null, null, null, null, null, false, 0L, List.of(), false); }
        @Override public boolean isClientSide() { return false; }
    }
    static final class Player extends ServerPlayer {
        ItemStack stack;
        Level world;
        PlayerAdvancements progress;
        int used;
        boolean creative;
        Player() { super(null, null, null, null); }
        @Override public ItemStack getItemBlockingWith() { return stack; }
        @Override public float getYHeadRot() { return 0; }
        @Override public Vec3 position() { return Vec3.ZERO; }
        @Override public ServerLevel level() { return world; }
        @Override public InteractionHand getUsedItemHand() { return InteractionHand.OFF_HAND; }
        @Override public boolean hasInfiniteMaterials() { return creative; }
        @Override public PlayerAdvancements getAdvancements() { return progress; }
        @Override public void awardStat(Stat<?> stat) {
            if (stat.getType() != Stats.ITEM_USED) throw new AssertionError(stat);
            used++;
        }
        @Override public void onEquippedItemBroken(ItemStack broken, EquipmentSlot slot) {}
        @Override protected void blockUsingItem(ServerLevel level, LivingEntity attacker,
                DamageSource source, float damage, boolean full) {}
    }
    static final class Arrow extends net.minecraft.world.entity.projectile.arrow.Arrow {
        boolean piercing;
        Arrow() { super(null, null); }
        @Override public byte getPierceLevel() { return (byte)(piercing ? 1 : 0); }
    }
    static final class Source extends DamageSource {
        final Vec3 from;
        Source(Arrow arrow, boolean behind) {
            super(net.minecraft.core.Holder.direct(
                new net.minecraft.world.damagesource.DamageType("arrow", 0)), arrow, null);
            from = new Vec3(0, 0, behind ? -1 : 1);
        }
        @Override public Vec3 getSourcePosition() { return from; }
    }
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var type = Class.forName("sun.misc.Unsafe");
        var field = type.getDeclaredField("theUnsafe");
        field.setAccessible(true);
        unsafe = field.get(null);
        allocate = type.getMethod("allocateInstance", Class.class);
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1])));
                var lines = Files.lines(Path.of(args[0]))) {
            int index = 0;
            for (var line : (Iterable<String>)lines::iterator) {
                var op = JsonParser.parseString(line).getAsJsonObject().getAsJsonObject("shield_hit");
                var player = make(Player.class);
                player.world = make(Level.class);
                player.stack = ParityData.stack(op.get("item").getAsString());
                player.creative = op.get("creative").getAsBoolean();
                player.progress = make(PlayerAdvancements.class);
                VanillaBlockUpdateOracle.set(PlayerAdvancements.class, player.progress, "activeTriggers", new HashMap<>());
                var arrow = make(Arrow.class);
                arrow.piercing = op.get("pierce").getAsBoolean();
                var source = new Source(arrow, op.get("behind").getAsBoolean());
                float damage = op.get("damage").getAsFloat();
                float blocked = player.creative ? damage : player.applyItemBlocking(player.world, source, damage);
                float health = 20 - (damage - blocked);
                out.println("case " + index++);
                out.printf(java.util.Locale.ROOT, "shield health=%.6f used=%d stack=%s%n",
                    health, player.used,
                    player.stack.isEmpty() ? "-" : player.stack.getCount() + ":" + player.stack.getDamageValue());
            }
        }
    }
}
