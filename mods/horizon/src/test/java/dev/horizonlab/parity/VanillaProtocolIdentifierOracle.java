package dev.horizonlab.parity;

import io.netty.buffer.Unpooled;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.charset.StandardCharsets;
import java.util.HexFormat;
import net.minecraft.network.FriendlyByteBuf;
import net.minecraft.resources.Identifier;

/** Identifier.STREAM_CODEC against raw packet bytes; no game/server bootstrap. */
public final class VanillaProtocolIdentifierOracle {
    public static void main(String[] args) throws Exception {
        try (var output = Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8)) {
            for (String line : Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8)) {
                var buffer = new FriendlyByteBuf(Unpooled.wrappedBuffer(HexFormat.of().parseHex(line)));
                try {
                    String value = Identifier.STREAM_CODEC.decode(buffer).toString();
                    output.write("OK " + buffer.readerIndex() + " ");
                    for (int i = 0; i < value.length(); i++) output.write(String.format("%04x", (int) value.charAt(i)));
                    output.newLine();
                } catch (RuntimeException error) {
                    output.write("ERR " + buffer.readerIndex());
                    output.newLine();
                } finally { buffer.release(); }
            }
        }
    }
}
