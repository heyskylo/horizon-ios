package dev.horizonlab.parity;

import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.nbt.DoubleTag;
import net.minecraft.nbt.FloatTag;

/** Actual pinned NBT visitors and JDK numeric text; no world or server. */
public final class VanillaFloatTextOracle {
    public static void main(String[] args) throws Exception {
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1])))) {
            for (String line : Files.readAllLines(Path.of(args[0]))) {
                String[] input = line.split(" ");
                if (input[0].equals("f")) {
                    float value = Float.intBitsToFloat(Integer.parseUnsignedInt(input[1], 16));
                    String text = Float.toString(value);
                    out.println(line + "\t" + text + "\t" + FloatTag.valueOf(value) + "\t" + text);
                } else {
                    double value = Double.longBitsToDouble(Long.parseUnsignedLong(input[1], 16));
                    String text = Double.toString(value);
                    out.println(line + "\t" + text + "\t" + DoubleTag.valueOf(value) + "\t" + text);
                }
            }
        }
    }
}
