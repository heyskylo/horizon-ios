package dev.horizonlab.parity;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Map;
import java.util.Set;
import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.Mth;
import net.minecraft.world.attribute.EnvironmentAttribute;
import net.minecraft.world.attribute.EnvironmentAttributeMap;
import net.minecraft.world.attribute.EnvironmentAttributeSystem;
import net.minecraft.world.attribute.GaussianSampler;
import net.minecraft.world.attribute.SpatialAttributeInterpolator;
import net.minecraft.world.attribute.WeatherAttributes;
import net.minecraft.world.phys.Vec3;
import org.joml.Vector3fc;
import org.joml.Vector4fc;

/**
 * Builds the pinned game's EnvironmentAttributeSystem per case (a dimension's attributes, the
 * biome layer, the weather layers) and prints every attribute's value at a position whose biome
 * mix comes from GaussianSampler over a test grid of biomes.
 */
public final class VanillaEnvironmentSystemOracle {
    private static final Set<String> PORTED = Set.of(
        "minecraft:boolean", "minecraft:float", "minecraft:angle_degrees", "minecraft:rgb_color", "minecraft:argb_color");
    private static final Map<String, EnvironmentAttributeMap> MAPS = new HashMap<>();

    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        var lines = Files.readAllLines(Path.of(args[0]));
        var data = JsonParser.parseString(lines.getFirst()).getAsJsonObject();
        for (var entry : data.entrySet()) {
            // Attributes of types not ported (music, sounds, mob spawns) need registries to parse
            // and don't affect the others: they are left out.
            var ported = new JsonObject();
            for (var attribute : entry.getValue().getAsJsonObject().entrySet()) {
                var type = BuiltInRegistries.ENVIRONMENT_ATTRIBUTE.getValue(Identifier.parse(attribute.getKey())).type();
                if (PORTED.contains(BuiltInRegistries.ATTRIBUTE_TYPE.getKey(type).toString())) {
                    ported.add(attribute.getKey(), attribute.getValue());
                }
            }
            MAPS.put(entry.getKey(), EnvironmentAttributeMap.CODEC.parse(JsonOps.INSTANCE, ported).getOrThrow());
        }
        var output = new ArrayList<String>();
        for (String line : lines.subList(1, lines.size())) {
            run(JsonParser.parseString(line).getAsJsonObject(), output);
        }
        Files.write(Path.of(args[1]), output);
    }

    /** The test grid: a cell's biome is one of `grid`, picked by a hash of its coordinates. */
    static String cell(List<String> grid, int x, int y, int z) {
        int hash = ((x * 31 + y) * 31 + z) & Integer.MAX_VALUE;
        return grid.get(hash % grid.size());
    }

    @SuppressWarnings("unchecked")
    private static void run(JsonObject c, List<String> output) {
        var grid = new ArrayList<String>();
        c.getAsJsonArray("grid").forEach(e -> grid.add(e.getAsString()));
        var at = c.getAsJsonArray("position");
        var position = new Vec3(at.get(0).getAsDouble(), at.get(1).getAsDouble(), at.get(2).getAsDouble());
        float rain = c.get("rain").getAsFloat();
        float thunder = c.get("thunder").getAsFloat();

        var builder = EnvironmentAttributeSystem.builder();
        builder.addConstantLayer(MAPS.get(c.get("dimension").getAsString()));
        // The biome layer of EnvironmentAttributeSystem.addBiomeLayerForAttribute, with the
        // biome at the position given by the grid instead of a BiomeManager.
        var here = MAPS.get(cell(grid, Mth.floor(position.x / 4), Mth.floor(position.y / 4), Mth.floor(position.z / 4)));
        var withBiomes = new LinkedHashSet<EnvironmentAttribute<?>>();
        c.getAsJsonArray("biomes").forEach(id -> withBiomes.addAll(MAPS.get(id.getAsString()).keySet()));
        for (var raw : withBiomes) {
            var attribute = (EnvironmentAttribute<Object>) raw;
            builder.addPositionalLayer(attribute, (base, pos, weights) ->
                weights != null && attribute.isSpatiallyInterpolated()
                    ? weights.applyAttributeLayer(attribute, base)
                    : here.applyModifier(attribute, base));
        }
        if (c.get("weather").getAsBoolean()) {
            WeatherAttributes.addBuiltinLayers(builder, new WeatherAttributes.WeatherAccess() {
                @Override
                public float rainLevel() {
                    return rain;
                }

                @Override
                public float thunderLevel() {
                    return thunder;
                }
            });
        }
        var system = builder.build();

        SpatialAttributeInterpolator blend = null;
        if (c.get("blend").getAsBoolean()) {
            var interpolator = new SpatialAttributeInterpolator();
            GaussianSampler.sample(position.scale(0.25), (x, y, z) -> MAPS.get(cell(grid, x, y, z)), interpolator::accumulate);
            blend = interpolator;
        }
        for (var raw : BuiltInRegistries.ENVIRONMENT_ATTRIBUTE) {
            if (!PORTED.contains(BuiltInRegistries.ATTRIBUTE_TYPE.getKey(raw.type()).toString())) {
                continue;
            }
            var attribute = (EnvironmentAttribute<Object>) raw;
            output.add(BuiltInRegistries.ENVIRONMENT_ATTRIBUTE.getKey(raw) + "\t" + format(system.getValue(attribute, position, blend))
                + "\t" + format(system.getDimensionValue(attribute)));
        }
    }

    private static String bits(float f) {
        return String.format("%08x", Float.floatToRawIntBits(f));
    }

    private static String format(Object value) {
        if (value instanceof Float f) {
            return bits(f);
        }
        if (value instanceof Vector4fc v) {
            return bits(v.x()) + " " + bits(v.y()) + " " + bits(v.z()) + " " + bits(v.w());
        }
        if (value instanceof Vector3fc v) {
            return bits(v.x()) + " " + bits(v.y()) + " " + bits(v.z());
        }
        return String.valueOf(value);
    }
}
