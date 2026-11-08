package dev.horizonlab.parity;

import com.google.gson.JsonParser;
import com.mojang.brigadier.StringReader;
import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.commands.arguments.item.ItemParser;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.level.block.state.BlockState;

/** Actual command admission, stack mining queries and saved-stack round trips. */
public final class VanillaStackMiningOracle {
    static String query(ItemStack stack, BlockState block) {
        var tool = stack.get(DataComponents.TOOL);
        return Integer.toUnsignedString(Float.floatToRawIntBits(stack.getDestroySpeed(block))) + " "
            + stack.isCorrectToolForDrops(block) + " "
            + (tool == null ? "none" : tool.damagePerBlock()) + " "
            + (stack.isDamageableItem() ? stack.getMaxDamage() : 0);
    }

    public static void main(String[] args) throws Exception {
        ParityData.init();
        var parser = new ItemParser(ParityData.lookup);
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1])));
             var lines = Files.lines(Path.of(args[0]))) {
            for (var line : (Iterable<String>) lines::iterator) {
                var row = JsonParser.parseString(line).getAsJsonObject();
                var stack = parser.parse(new StringReader(row.get("item").getAsString())).createItemStack(1);
                if (row.has("count")) stack.setCount(row.get("count").getAsInt());
                var block = BuiltInRegistries.BLOCK.getValue(Identifier.parse(row.get("block").getAsString())).defaultBlockState();
                var restored = stack;
                if (!stack.isEmpty()) {
                    var saved = ItemStack.CODEC.encodeStart(ParityData.ops, stack).getOrThrow();
                    restored = ItemStack.CODEC.parse(ParityData.ops, saved).getOrThrow();
                }
                out.println(query(stack, block) + " | " + query(restored, block));
            }
        }
    }
}
