package dev.horizonlab.parity;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.client.renderer.item.properties.conditional.CustomModelDataProperty;
import net.minecraft.core.component.DataComponents;
import net.minecraft.nbt.NbtOps;
import net.minecraft.nbt.TagParser;
import net.minecraft.world.item.ItemDisplayContext;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;

/** Actual component codec and client conditional property, without graphics. */
public final class VanillaStackModelFlagsOracle {
    public static void main(String[] args) throws Exception {
        ParityData.init();
        try (var out = Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8)) {
            for (var line : Files.readAllLines(Path.of(args[0]), StandardCharsets.UTF_8)) {
                var input = JsonParser.parseString(line).getAsJsonObject();
                var codec = DataComponents.CUSTOM_MODEL_DATA.codec();
                var tag = TagParser.parseCompoundFully(input.get("nbt").getAsString());
                var data = codec.parse(NbtOps.INSTANCE, tag).getOrThrow();
                var stack = new ItemStack(Items.DIAMOND);
                stack.set(DataComponents.CUSTOM_MODEL_DATA, data);
                var property = new CustomModelDataProperty(input.get("index").getAsInt());
                boolean selected = property.get(stack, null, null, 0, ItemDisplayContext.GUI);
                var result = new JsonObject();
                result.add("id", input.get("id"));
                result.add("component", codec.encodeStart(JsonOps.INSTANCE, data).getOrThrow());
                result.addProperty("selected", selected ? "horizon:item/yes" : "horizon:item/no");
                out.write(result.toString());
                out.newLine();
            }
        }
    }
}
