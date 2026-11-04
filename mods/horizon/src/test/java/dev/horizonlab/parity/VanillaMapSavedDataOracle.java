package dev.horizonlab.parity;

import com.google.gson.JsonParser;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.TreeSet;
import net.minecraft.nbt.*;
import net.minecraft.world.level.saveddata.maps.MapItemSavedData;

/** Original transport for the real saved-map CODEC, without world or menu simulation. */
public final class VanillaMapSavedDataOracle {
    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        var lines = new ArrayList<String>();
        for (var element : JsonParser.parseString(Files.readString(Path.of(args[0]))).getAsJsonArray()) {
            var row = element.getAsJsonObject();
            var input = new CompoundTag();
            input.putString("dimension", row.get("dimension").getAsString());
            input.putInt("xCenter", row.get("x").getAsInt());
            input.putInt("zCenter", row.get("z").getAsInt());
            input.putByte("scale", row.get("scale").getAsByte());
            input.putBoolean("locked", row.get("locked").getAsBoolean());
            input.putBoolean("trackingPosition", row.get("tracking").getAsBoolean());
            input.putBoolean("unlimitedTracking", row.get("unlimited").getAsBoolean());
            byte[] colors = new byte[128 * 128];
            for (int i = 0; i < colors.length; i++) colors[i] = (byte)(i * 17);
            input.putByteArray("colors", colors);
            var banners = new ListTag();
            if (row.get("banner").getAsBoolean()) {
                var banner = new CompoundTag();
                banner.putIntArray("pos", new int[]{7, 70, -11});
                banner.putString("color", "red");
                banners.add(banner);
            }
            input.put("banners", banners);
            var frames = new ListTag();
            if (row.get("frame").getAsBoolean()) {
                var frame = new CompoundTag();
                frame.putIntArray("pos", new int[]{-3, 80, 9});
                frame.putInt("rotation", 270);
                frame.putInt("entity_id", 42);
                frames.add(frame);
            }
            input.put("frames", frames);
            var map = MapItemSavedData.CODEC.parse(NbtOps.INSTANCE, input).getOrThrow();
            var saved = (CompoundTag)MapItemSavedData.CODEC.encodeStart(NbtOps.INSTANCE, map).getOrThrow();
            var expected = new TreeSet<>(List.of("dimension", "xCenter", "zCenter", "colors"));
            if (row.get("scale").getAsInt() != 0) expected.add("scale");
            if (row.get("locked").getAsBoolean()) expected.add("locked");
            if (!row.get("tracking").getAsBoolean()) expected.add("trackingPosition");
            if (row.get("unlimited").getAsBoolean()) expected.add("unlimitedTracking");
            if (row.get("banner").getAsBoolean()) expected.add("banners");
            if (row.get("frame").getAsBoolean()) expected.add("frames");
            if (!saved.keySet().equals(expected)) throw new AssertionError(row.toString());
            for (var key : expected)
                if (!saved.get(key).equals(input.get(key))) throw new AssertionError(key + ": " + row);
            lines.add(row.get("name").getAsString() + " " + ParityData.canon(saved));
        }
        Files.write(Path.of(args[1]), lines);
    }
}
