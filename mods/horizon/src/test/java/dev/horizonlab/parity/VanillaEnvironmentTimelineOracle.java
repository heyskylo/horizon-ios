package dev.horizonlab.parity;

import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.Optional;
import net.minecraft.SharedConstants;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Bootstrap;
import net.minecraft.util.EasingType;
import net.minecraft.util.KeyframeTrackSampler;
import net.minecraft.world.attribute.EnvironmentAttribute;
import net.minecraft.world.attribute.modifier.AttributeModifier;
import net.minecraft.world.timeline.AttributeTrack;
import org.joml.Vector3fc;
import org.joml.Vector4fc;

/**
 * Easings through EasingType.CODEC, and attribute tracks through AttributeTrack's codec and
 * validatePeriod, sampled by KeyframeTrackSampler at given ticks and applied by their modifier
 * (what AttributeTrackSampler.applyTimeBased does with the clock's total ticks).
 */
public final class VanillaEnvironmentTimelineOracle {
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        var output = new ArrayList<String>();
        for (String line : Files.readAllLines(Path.of(args[0]))) {
            String[] f = line.split("\t");
            output.add(switch (f[0]) {
                case "ease" -> ease(f[1], Float.parseFloat(f[2]));
                case "track" -> track(f[1], f[2], f[3], f[4], f[5]);
                default -> throw new IllegalArgumentException(line);
            });
        }
        Files.write(Path.of(args[1]), output);
    }

    private static String ease(String json, float x) {
        var easing = EasingType.CODEC.parse(JsonOps.INSTANCE, JsonParser.parseString(json));
        return easing.isError() ? "error" : bits(easing.getOrThrow().apply(x));
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    private static String track(String id, String period, String trackJson, String baseJson, String ticks) {
        var attribute = (EnvironmentAttribute<Object>) BuiltInRegistries.ENVIRONMENT_ATTRIBUTE.getValue(Identifier.parse(id));
        Object base = attribute.valueCodec().parse(JsonOps.INSTANCE, JsonParser.parseString(baseJson)).getOrThrow();
        var parsed = AttributeTrack.createCodec(attribute).parse(JsonOps.INSTANCE, JsonParser.parseString(trackJson));
        Optional<Integer> periodTicks = period.equals("-") ? Optional.empty() : Optional.of(Integer.parseInt(period));
        if (parsed.isError()) {
            return "error";
        }
        AttributeTrack track = parsed.getOrThrow();
        if (periodTicks.isPresent() && AttributeTrack.validatePeriod(track, periodTicks.get()).isError()) {
            return "error";
        }
        AttributeModifier modifier = track.modifier();
        KeyframeTrackSampler sampler = track.argumentTrack().bakeSampler(periodTicks, modifier.argumentKeyframeLerp(attribute));
        var out = new StringBuilder();
        for (String t : ticks.split(",")) {
            if (!out.isEmpty()) {
                out.append(';');
            }
            out.append(format(modifier.apply(base, sampler.sample(Long.parseLong(t)))));
        }
        return out.toString();
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
