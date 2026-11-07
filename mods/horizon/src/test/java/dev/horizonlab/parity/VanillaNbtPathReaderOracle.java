package dev.horizonlab.parity;

import com.google.gson.JsonParser;
import com.mojang.brigadier.StringReader;
import com.mojang.brigadier.exceptions.CommandSyntaxException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.commands.arguments.NbtPathArgument;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.contents.TranslatableContents;

/** Real NbtPathArgument parsing only: no game bootstrap, resources or server. */
public final class VanillaNbtPathReaderOracle {
    public static void main(String[] args) throws Exception {
        try (var out = Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8)) {
            for (String line : Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8)) {
                String text = JsonParser.parseString(line).getAsString();
                StringReader reader = new StringReader(text);
                try {
                    var path = NbtPathArgument.nbtPath().parse(reader);
                    out.write("OK " + utf8Cursor(text, reader.getCursor()) + " ");
                    for (byte value : path.toString().getBytes(StandardCharsets.UTF_8))
                        out.write(String.format("%02x", value & 255));
                } catch (CommandSyntaxException error) {
                    var built = CommandSyntaxException.BUILT_IN_EXCEPTIONS;
                    String key = error.getType() == built.readerInvalidEscape() ? "parsing.quote.escape"
                        : error.getType() == built.readerExpectedEndOfQuote() ? "parsing.quote.expected.end"
                        : error.getType() == built.readerInvalidInt() ? "parsing.int.invalid"
                        : error.getType() == built.readerExpectedInt() ? "parsing.int.expected"
                        : error.getType() == built.readerExpectedSymbol() ? "parsing.expected"
                        : error.getRawMessage() instanceof Component component
                            && component.getContents() instanceof TranslatableContents contents
                            ? contents.getKey() : "unknown";
                    out.write("ERR " + utf8Cursor(text, reader.getCursor()) + " " + key);
                }
                out.newLine();
            }
        }
    }
    private static int utf8Cursor(String text, int cursor) {
        return text.substring(0, cursor).getBytes(StandardCharsets.UTF_8).length;
    }
}
