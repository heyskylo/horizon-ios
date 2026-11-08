package dev.horizonlab.parity;

import io.netty.buffer.Unpooled;
import java.io.PrintWriter;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HexFormat;
import net.minecraft.network.Utf8String;

/** Calls the real packet string codec, including its error cursor positions. */
public final class VanillaProtocolStringsOracle {
    public static void main(String[] args) throws Exception {
        try (var input = Files.newBufferedReader(Path.of(args[0]));
             var output = new PrintWriter(Files.newBufferedWriter(Path.of(args[1])))) {
            String line;
            while ((line = input.readLine()) != null) {
                int split = line.indexOf(' ');
                int limit = Integer.parseInt(line.substring(0, split));
                var buffer = Unpooled.wrappedBuffer(HexFormat.of().parseHex(line.substring(split + 1)));
                try {
                    String value = Utf8String.read(buffer, limit);
                    output.print("OK " + buffer.readerIndex() + " ");
                    for (char unit : value.toCharArray()) {
                        output.printf("%04x", (int) unit);
                    }
                    output.println();
                } catch (RuntimeException error) {
                    output.println("ERR " + buffer.readerIndex());
                } finally {
                    buffer.release();
                }
            }
        }
    }
}
