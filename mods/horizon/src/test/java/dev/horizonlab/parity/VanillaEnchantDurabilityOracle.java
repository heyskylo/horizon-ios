package dev.horizonlab.parity;

import com.google.gson.*;
import java.io.*;
import java.nio.file.*;
import net.minecraft.core.registries.*;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.util.RandomSource;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.enchantment.Enchantment;
import net.minecraft.world.item.enchantment.EnchantmentHelper;
import net.minecraft.core.component.DataComponents;

/**
 * `EnchantmentHelper.processDurabilityChange` (Unbreaking) and
 * `modifyDurabilityToRepairFromXp` (Mending) on a real enchantment registry,
 * one item stack and seeded level random per case.
 */
public final class VanillaEnchantDurabilityOracle {
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var allocator = Class.forName("sun.misc.Unsafe");
        var unsafeField = allocator.getDeclaredField("theUnsafe");
        unsafeField.setAccessible(true);
        VanillaBlockUpdateOracle.unsafe = unsafeField.get(null);
        VanillaBlockUpdateOracle.allocate = allocator.getMethod("allocateInstance", Class.class);
        VanillaBlockUpdateOracle.AIR = net.minecraft.world.level.block.Blocks.AIR.defaultBlockState();
        var empty = new JsonObject();
        empty.add("cells", new JsonArray());
        empty.addProperty("seed", 0L);
        var level = VanillaBlockUpdateOracle.build(empty);
        level.registries = ParityData.access;
        level.server.levels = java.util.Map.of(net.minecraft.world.level.Level.OVERWORLD, level);
        VanillaBlockUpdateOracle.set(net.minecraft.world.level.Level.class, level, "random", RandomSource.create(0L));
        var enchantments = ParityData.lookup.lookupOrThrow(Registries.ENCHANTMENT);
        var unbreaking = enchantments.getOrThrow(ResourceKey.create(Registries.ENCHANTMENT, Identifier.parse("minecraft:unbreaking")));
        var mending = enchantments.getOrThrow(ResourceKey.create(Registries.ENCHANTMENT, Identifier.parse("minecraft:mending")));
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1]))); var lines = Files.newBufferedReader(Path.of(args[0]))) {
            String line;
            while ((line = lines.readLine()) != null) {
                if (line.startsWith("#resources ")) continue;
                var f = JsonParser.parseString(line).getAsJsonObject();
                var item = BuiltInRegistries.ITEM.getValue(Identifier.parse(f.get("item").getAsString()));
                var stack = new ItemStack(item);
                stack.set(DataComponents.DAMAGE, f.get("damage").getAsInt());
                String mode = f.get("mode").getAsString();
                if (mode.equals("unbreaking")) {
                    stack.enchant(unbreaking, f.get("level").getAsInt());
                    level.rng = RandomSource.create(f.get("seed").getAsLong());
                    out.println(EnchantmentHelper.processDurabilityChange(level, stack, f.get("amount").getAsInt()));
                } else {
                    stack.enchant(mending, f.get("level").getAsInt());
                    out.println(EnchantmentHelper.modifyDurabilityToRepairFromXp(level, stack, f.get("amount").getAsInt()));
                }
            }
        }
        System.exit(0);
    }
}
