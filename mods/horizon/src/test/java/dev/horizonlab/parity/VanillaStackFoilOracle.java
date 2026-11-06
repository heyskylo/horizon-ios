package dev.horizonlab.parity;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.nbt.TagParser;
import net.minecraft.nbt.NbtOps;
import net.minecraft.resources.Identifier;
import net.minecraft.world.item.ItemStack;

/** NBT boolean components through the actual codec, JSON encoding and ItemStack.hasFoil. */
public final class VanillaStackFoilOracle {
    public static void main(String[] args) throws Exception {
        ParityData.init();
        try (var out = Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8)) {
            for (var line : Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8)) {
                var input = JsonParser.parseString(line).getAsJsonObject();
                var stack = new ItemStack(BuiltInRegistries.ITEM.getValue(Identifier.parse(input.get("item").getAsString())));
                var result = new JsonObject();
                result.add("id", input.get("id"));
                if (input.has("nbt")) {
                    var codec = DataComponents.ENCHANTMENT_GLINT_OVERRIDE.codec();
                    var tag = TagParser.parseCompoundFully("{v:" + input.get("nbt").getAsString() + "}").get("v");
                    boolean flag = codec.parse(NbtOps.INSTANCE, tag).getOrThrow();
                    stack.set(DataComponents.ENCHANTMENT_GLINT_OVERRIDE, flag);
                    result.add("component", codec.encodeStart(JsonOps.INSTANCE, flag).getOrThrow());
                } else {
                    result.add("component", com.google.gson.JsonNull.INSTANCE);
                }
                result.addProperty("foil", stack.hasFoil());
                out.write(result.toString());
                out.newLine();
            }
        }
    }
}
