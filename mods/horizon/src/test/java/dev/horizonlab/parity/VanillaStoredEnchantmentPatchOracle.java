package dev.horizonlab.parity;

import com.mojang.brigadier.StringReader;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.commands.arguments.item.ItemParser;
import net.minecraft.core.component.DataComponents;

/** Actual command construction and saved ItemStack codecs, without a server or world. */
public final class VanillaStoredEnchantmentPatchOracle {
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var parser = new ItemParser(ParityData.lookup);
        try (var output = Files.newBufferedWriter(Path.of(args[1]))) {
            for (var line : Files.readAllLines(Path.of(args[0]))) {
                var parts = line.split("\t", 3);
                if (parts.length != 3) throw new IllegalArgumentException("expected label, item argument and saved stack");
                var command = parser.parse(new StringReader(parts[1])).createItemStack(1);
                var decoded = ParityData.stack(parts[2]);
                output.write(parts[0] + "\t" + ParityData.canon(command) + "\t" + ParityData.canon(decoded)
                    + "\t" + command.has(DataComponents.STORED_ENCHANTMENTS)
                    + "\t" + decoded.has(DataComponents.STORED_ENCHANTMENTS) + "\n");
            }
        }
    }
}
