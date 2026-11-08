package dev.horizonlab.parity;

import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.SharedConstants;
import net.minecraft.core.*;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.RegistryDataLoader;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.Bootstrap;
import net.minecraft.server.RegistryLayer;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.repository.ServerPacksSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.tags.EnchantmentTags;
import net.minecraft.tags.TagLoader;
import net.minecraft.util.RandomSource;
import net.minecraft.core.component.DataComponents;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.item.enchantment.Enchantment;
import net.minecraft.world.item.enchantment.EnchantmentHelper;
import net.minecraft.world.item.enchantment.EnchantmentInstance;
import net.minecraft.world.item.enchantment.ItemEnchantments;

/**
 * Original oracle: loads the vanilla data pack's enchantment registry and
 * replays the enchanting table offer computation (EnchantmentMenu.slotsChanged
 * and getEnchantmentList) through EnchantmentHelper for fixture lines
 * "seed bookcases item damage enchantments", printing costs, clues and the
 * enchantments applied. The stack carries the case's damage and
 * minecraft:enchantments, and the isEnchantable gate is applied first.
 */
public final class VanillaEnchantingOracle {
    private static String name(EnchantmentInstance e) {
        return e.enchantment().unwrapKey().get().identifier() + ":" + e.level();
    }

    private static List<EnchantmentInstance> list(RandomSource random, int seed, int slot, int cost, ItemStack stack, HolderSet.Named<Enchantment> table) {
        random.setSeed(seed + slot);
        List<EnchantmentInstance> list = EnchantmentHelper.selectEnchantment(random, stack, cost, table.stream());
        if (stack.is(Items.BOOK) && list.size() > 1) {
            list.remove(random.nextInt(list.size()));
        }
        return list;
    }

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        Path in = Path.of(args[0]), out = Path.of(args[1]);
        var repo = ServerPacksSource.createVanillaTrustedRepository();
        repo.reload();
        repo.setSelected(List.of("vanilla"));
        var resources = new MultiPackResourceManager(PackType.SERVER_DATA, repo.openAllSelected());
        var layers = RegistryLayer.createRegistryAccess();
        var staticTags = TagLoader.loadTagsForExistingRegistries(resources, layers.getLayer(RegistryLayer.STATIC));
        var context = TagLoader.buildUpdatedLookups(layers.getAccessForLoading(RegistryLayer.WORLD), staticTags);
        RegistryAccess.Frozen world = RegistryDataLoader.load(resources, context, RegistryDataLoader.WORLD_REGISTRIES, Runnable::run).join();
        staticTags.forEach(Registry.PendingTags::apply);
        RegistryAccess access = layers.replaceFrom(RegistryLayer.WORLD, world).compositeAccess();
        // Bind default item components (enchantable values) as server reloads do.
        BuiltInRegistries.DATA_COMPONENT_INITIALIZERS.build(access).forEach(p -> p.apply());
        var table = world.lookupOrThrow(Registries.ENCHANTMENT).get(EnchantmentTags.IN_ENCHANTING_TABLE).orElseThrow();
        RandomSource random = RandomSource.create(0L);
        try (var w = new PrintWriter(Files.newBufferedWriter(out, StandardCharsets.UTF_8))) {
            for (String line : Files.readAllLines(in, StandardCharsets.UTF_8)) {
                String[] f = line.split(" ");
                int seed = Integer.parseInt(f[0]), bookcases = Integer.parseInt(f[1]);
                ItemStack stack = new ItemStack(BuiltInRegistries.ITEM.getValue(Identifier.parse(f[2])));
                if (f.length > 3) stack.setDamageValue(Integer.parseInt(f[3]));
                if (f.length > 4 && !f[4].equals("-")) {
                    var levels = new ItemEnchantments.Mutable(ItemEnchantments.EMPTY);
                    var enchants = access.lookupOrThrow(Registries.ENCHANTMENT);
                    for (String part : f[4].split(",")) {
                        String[] kv = part.split("=");
                        levels.set(enchants.getOrThrow(ResourceKey.create(Registries.ENCHANTMENT, Identifier.parse(kv[0]))),
                                   Integer.parseInt(kv[1]));
                    }
                    stack.set(DataComponents.ENCHANTMENTS, levels.toImmutable());
                }
                int[] costs = new int[3];
                if (!stack.isEnchantable()) {
                    for (int i = 0; i < 3; i++) w.println(line + " " + i + " 0 - -");
                    continue;
                }
                random.setSeed(seed);
                for (int i = 0; i < 3; i++) {
                    costs[i] = EnchantmentHelper.getEnchantmentCost(random, i, bookcases, stack);
                    if (costs[i] < i + 1) costs[i] = 0;
                }
                for (int i = 0; i < 3; i++) {
                    String clue = "-";
                    String applied = "-";
                    if (costs[i] > 0) {
                        List<EnchantmentInstance> l = list(random, seed, i, costs[i], stack, table);
                        if (!l.isEmpty()) clue = name(l.get(random.nextInt(l.size())));
                        List<EnchantmentInstance> again = list(RandomSource.create(0L), seed, i, costs[i], stack, table);
                        StringBuilder sb = new StringBuilder();
                        for (var e : again) sb.append(sb.length() == 0 ? "" : ",").append(name(e));
                        if (sb.length() > 0) applied = sb.toString();
                    }
                    w.println(line + " " + i + " " + costs[i] + " " + clue + " " + applied);
                }
            }
        }
    }
}
