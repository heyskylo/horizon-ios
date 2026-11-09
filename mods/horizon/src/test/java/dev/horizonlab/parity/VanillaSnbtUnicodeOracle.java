package dev.horizonlab.parity;

import com.google.gson.JsonElement;
import com.google.gson.JsonParser;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HexFormat;
import net.minecraft.nbt.CompoundTag;
import net.minecraft.nbt.IntTag;
import net.minecraft.nbt.Tag;

/** Actual CompoundTag/StringTagVisitor output, without a world or server. */
public final class VanillaSnbtUnicodeOracle {
    private static Tag tag(JsonElement value) {
        if (value.isJsonPrimitive()) return IntTag.valueOf(value.getAsInt());
        var compound = new CompoundTag();
        for (var raw : value.getAsJsonObject().getAsJsonArray("entries")) {
            var entry = raw.getAsJsonArray();
            compound.put(entry.get(0).getAsString(), tag(entry.get(1)));
        }
        return compound;
    }

    public static void main(String[] args) throws Exception {
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1])))) {
            for (String line : Files.readAllLines(Path.of(args[0]))) {
                String text = tag(JsonParser.parseString(line)).toString();
                // Fixture keys are valid Unicode; encode the exact UTF-16 units.
                out.println(HexFormat.of().formatHex(text.getBytes(StandardCharsets.UTF_16BE)));
            }
        }
    }
}
