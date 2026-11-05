package dev.horizonlab.parity;

import com.google.gson.*;
import com.mojang.serialization.JsonOps;
import java.io.*;
import net.minecraft.world.damagesource.DamageType;

/** Calls the pinned jar's codec, including its optional/default fields. */
public final class VanillaDamageCatalogOracle {
    public static void main(String[] args) throws Exception {
        var output = System.out;
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        try (var input = new BufferedReader(new InputStreamReader(System.in))) {
            String line;
            while ((line = input.readLine()) != null) {
                var result = DamageType.DIRECT_CODEC.parse(JsonOps.INSTANCE, JsonParser.parseString(line)).result();
                if (result.isEmpty()) {
                    output.println("false");
                    continue;
                }
                var d = result.orElseThrow();
                var out = new JsonArray();
                out.add(d.msgId());
                out.add(d.scaling().getSerializedName());
                out.add(String.format("%08x", Float.floatToIntBits(d.exhaustion())));
                out.add(d.effects().getSerializedName());
                out.add(d.deathMessageType().getSerializedName());
                var nbt = (net.minecraft.nbt.CompoundTag) DamageType.DIRECT_CODEC.encodeStart(net.minecraft.nbt.NbtOps.INSTANCE, d).getOrThrow();
                out.add(String.format("%08x", Float.floatToIntBits(((net.minecraft.nbt.FloatTag) nbt.get("exhaustion")).floatValue())));
                out.add(nbt.contains("effects"));
                out.add(nbt.contains("death_message_type"));
                output.println(out);
            }
        }
    }
}
