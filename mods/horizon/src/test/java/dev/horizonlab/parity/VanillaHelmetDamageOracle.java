package dev.horizonlab.parity;

import com.google.gson.*;
import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import net.minecraft.core.*;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.*;
import net.minecraft.resources.Identifier;
import net.minecraft.server.level.*;
import net.minecraft.tags.DamageTypeTags;
import net.minecraft.util.RandomSource;
import net.minecraft.world.damagesource.*;
import net.minecraft.world.entity.*;
import net.minecraft.world.entity.ai.attributes.*;
import net.minecraft.world.entity.player.Player;
import net.minecraft.world.item.*;
import net.minecraft.world.item.enchantment.*;

/** Actual head/ordinary equipment durability, modifier removal and armor reduction.
 * Outer guards, cooldown, effects, health storage and player notifications are
 * covered separately; this oracle isolates the two vanilla equipment boundaries.
 */
public final class VanillaHelmetDamageOracle {
    static Object unsafe;
    static Method allocate;
    static <T> T make(Class<T> type) throws Exception {
        return type.cast(allocate.invoke(unsafe, type));
    }
    static final class Level extends ServerLevel {
        RandomSource random;
        VanillaBlockUpdateOracle.Server server;
        Level() { super(null, null, null, null, null, null, false, 0L, List.of(), false); }
        @Override public void broadcastEntityEvent(Entity entity, byte event) {}
        @Override public RandomSource getRandom() { return random; }
        @Override public VanillaBlockUpdateOracle.Server getServer() { return server; }
        @Override public RegistryAccess registryAccess() { return ParityData.access; }
    }
    static final class Actor extends Player {
        Level level;
        EnumMap<EquipmentSlot, ItemStack> gear;
        AttributeMap attributes;
        Actor() { super(null, null); }
        @Override public net.minecraft.world.level.GameType gameMode() {
            return net.minecraft.world.level.GameType.SURVIVAL;
        }
        @Override public Level level() { return level; }
        @Override public ItemStack getItemBySlot(EquipmentSlot slot) {
            return gear.getOrDefault(slot, ItemStack.EMPTY);
        }
        @Override public AttributeMap getAttributes() { return attributes; }
        @Override public double getAttributeValue(Holder<Attribute> attribute) {
            return attributes.getValue(attribute);
        }
        float hit(DamageSource source, float damage) {
            // LivingEntity.hurtServer's equipment boundary, followed by the
            // real Player.getDamageAfterArmorAbsorb and doHurtEquipment.
            if (source.is(DamageTypeTags.DAMAGES_HELMET) && !getItemBySlot(EquipmentSlot.HEAD).isEmpty()) {
                super.hurtHelmet(source, damage);
                damage *= 0.75F;
            }
            return super.getDamageAfterArmorAbsorb(source, damage);
        }
    }
    static ItemStack stack(String item, int damage, int unbreaking) {
        ItemStack stack = new ItemStack(BuiltInRegistries.ITEM.getValue(Identifier.parse("minecraft:" + item)));
        stack.setDamageValue(damage);
        if (unbreaking > 0) {
            var levels = new ItemEnchantments.Mutable(ItemEnchantments.EMPTY);
            levels.set(ParityData.lookup.lookupOrThrow(Registries.ENCHANTMENT).getOrThrow(Enchantments.UNBREAKING), unbreaking);
            stack.set(DataComponents.ENCHANTMENTS, levels.toImmutable());
        }
        return stack;
    }
    static int damage(Actor actor, EquipmentSlot slot) {
        ItemStack stack = actor.getItemBySlot(slot);
        return stack.isEmpty() ? -1 : stack.getDamageValue();
    }
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var unsafeField = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        unsafeField.setAccessible(true);
        unsafe = unsafeField.get(null);
        allocate = unsafe.getClass().getMethod("allocateInstance", Class.class);
        var attributesField = LivingEntity.class.getDeclaredField("attributes");
        attributesField.setAccessible(true);
        var locationEffects = LivingEntity.class.getDeclaredField("activeLocationDependentEnchantments");
        locationEffects.setAccessible(true);
        var cases = JsonParser.parseString(Files.readString(Path.of(args[0]))).getAsJsonArray();
        try (var out = Files.newBufferedWriter(Path.of(args[1]))) {
            for (int index = 0; index < cases.size(); index++) {
                var input = cases.get(index).getAsJsonObject();
                Actor actor = make(Actor.class);
                actor.level = make(Level.class);
                actor.level.random = RandomSource.create(input.get("seed").getAsLong());
                actor.level.server = make(VanillaBlockUpdateOracle.Server.class);
                actor.level.server.loot = new VanillaBlockUpdateOracle.Loot();
                actor.level.server.loot.delegate = ParityData.loot;
                actor.gear = new EnumMap<>(EquipmentSlot.class);
                actor.attributes = new AttributeMap(Player.createAttributes().build());
                attributesField.set(actor, actor.attributes);
                locationEffects.set(actor, new EnumMap<>(EquipmentSlot.class));
                int unbreaking = input.get("unbreaking").getAsInt();
                if (!input.get("head").isJsonNull()) {
                    actor.gear.put(EquipmentSlot.HEAD, stack(input.get("head").getAsString(), input.get("head_damage").getAsInt(), unbreaking));
                }
                if (!input.get("boots_damage").isJsonNull()) {
                    actor.gear.put(EquipmentSlot.FEET, stack("iron_boots", input.get("boots_damage").getAsInt(), unbreaking));
                }
                if (input.has("unbreakable") && input.get("unbreakable").getAsBoolean()) {
                    actor.gear.get(EquipmentSlot.HEAD).set(DataComponents.UNBREAKABLE, net.minecraft.util.Unit.INSTANCE);
                }
                for (var entry : actor.gear.entrySet()) {
                    entry.getValue().forEachModifier(entry.getKey(), (attribute, modifier) -> actor.attributes.getInstance(attribute).addTransientModifier(modifier));
                }
                var source = new DamageSource(ParityData.lookup.lookupOrThrow(Registries.DAMAGE_TYPE)
                    .getOrThrow(net.minecraft.resources.ResourceKey.create(Registries.DAMAGE_TYPE, Identifier.parse("minecraft:" + input.get("source").getAsString()))));
                float health = 20.0F - actor.hit(source, 8.0F);
                long next = actor.level.random.nextLong();
                for (int path = 0; path < 2; path++) {
                    out.write("HELMET " + (index * 2 + path) + " " + damage(actor, EquipmentSlot.HEAD) + " " + damage(actor, EquipmentSlot.FEET)
                        + " " + String.format("%08x", Float.floatToIntBits(health)) + " " + next);
                    out.newLine();
                }
            }
        }
    }
}
