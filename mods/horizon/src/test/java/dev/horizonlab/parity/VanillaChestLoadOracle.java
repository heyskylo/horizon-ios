package dev.horizonlab.parity;

import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.core.BlockPos;
import net.minecraft.core.NonNullList;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.nbt.TagParser;
import net.minecraft.util.ProblemReporter;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.ChestBlockEntity;
import net.minecraft.world.level.storage.TagValueInput;

/** Original oracle: load real chest NBT without opening or unpacking its loot. */
public final class VanillaChestLoadOracle {
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var itemsField = ChestBlockEntity.class.getDeclaredField("items");
        itemsField.setAccessible(true);
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8));
             var lines = Files.lines(Path.of(args[0]))) {
            for (String line : (Iterable<String>) lines::iterator) {
                var chest = new ChestBlockEntity(BlockPos.ZERO, Blocks.CHEST.defaultBlockState());
                chest.setItem(0, new ItemStack(Items.GOLD_INGOT, 5));
                chest.loadWithComponents(TagValueInput.create(ProblemReporter.DISCARDING,
                    ParityData.lookup, TagParser.parseCompoundFully(line)));
                @SuppressWarnings("unchecked")
                var slots = (NonNullList<ItemStack>) itemsField.get(chest);
                var result = new StringBuilder();
                for (int i = 0; i < slots.size(); i++) {
                    var stack = slots.get(i);
                    if (!stack.isEmpty()) {
                        result.append(i).append(':').append(BuiltInRegistries.ITEM.getKey(stack.getItem()))
                            .append(':').append(stack.getCount()).append(';');
                    }
                }
                out.println(result.isEmpty() ? "empty" : result);
            }
        }
    }
}
