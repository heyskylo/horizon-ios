package dev.horizonlab.parity;

import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.Registries;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.util.ProblemReporter;
import net.minecraft.util.RandomSource;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.ChestBlockEntity;
import net.minecraft.world.level.storage.TagValueInput;

/** Original oracle: unpack real loot, empty/replace contents, save and reload a chest. */
public final class VanillaOpenedLootOracle {
    static final class Chest extends ChestBlockEntity {
        Chest() { super(BlockPos.ZERO, Blocks.CHEST.defaultBlockState()); }
        // Persistence is under test; no real chunk receives dirty notifications.
        @Override public void setChanged() { }
    }
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var allocator = Class.forName("sun.misc.Unsafe");
        var field = allocator.getDeclaredField("theUnsafe");
        field.setAccessible(true);
        VanillaBlockUpdateOracle.unsafe = field.get(null);
        VanillaBlockUpdateOracle.allocate = allocator.getMethod("allocateInstance", Class.class);
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8))) {
            for (var line : Files.readAllLines(Path.of(args[0]))) {
                var parts = line.split(" ");
                long seed = Long.parseLong(parts[1]);
                int mode = Integer.parseInt(parts[2]);
                var world = VanillaBlockUpdateOracle.make(VanillaBlockUpdateOracle.World.class);
                world.registries = ParityData.access;
                world.cells = new java.util.HashMap<>();
                VanillaBlockUpdateOracle.AIR = Blocks.AIR.defaultBlockState();
                world.rng = RandomSource.create(seed);
                VanillaBlockUpdateOracle.set(Level.class, world, "random", world.rng);
                world.log = new StringBuilder();
                world.server = VanillaBlockUpdateOracle.make(VanillaBlockUpdateOracle.Server.class);
                world.server.loot = VanillaBlockUpdateOracle.make(VanillaBlockUpdateOracle.Loot.class);
                world.server.loot.world = world;
                world.server.loot.delegate = ParityData.loot;
                var chest = new Chest();
                chest.setLevel(world);
                chest.setLootTable(ResourceKey.create(Registries.LOOT_TABLE, Identifier.parse(parts[0])));
                chest.setLootTableSeed(seed);
                if (mode != 0) {
                    chest.unpackLootTable(null);
                    if (chest.getLootTable() != null) throw new AssertionError("unpack did not consume table");
                    chest.clearContent();
                    if (mode == 2) chest.setItem(0, new ItemStack(Items.GOLD_INGOT, 1));
                }
                var saved = chest.saveWithFullMetadata(ParityData.access);
                var reloaded = new Chest();
                reloaded.loadWithComponents(TagValueInput.create(ProblemReporter.DISCARDING, ParityData.lookup, saved));
                var items = ChestBlockEntity.class.getDeclaredField("items");
                items.setAccessible(true);
                @SuppressWarnings("unchecked")
                var slots = (net.minecraft.core.NonNullList<ItemStack>) items.get(reloaded);
                int count = slots.stream().mapToInt(ItemStack::getCount).sum();
                out.println((saved.contains("LootTable") ? 1 : 0) + " "
                    + (saved.contains("LootTableSeed") ? 1 : 0) + " " + (mode == 0 ? "unopened" : count));
            }
        }
    }
}
