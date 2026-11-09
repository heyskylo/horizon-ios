package dev.horizonlab.parity;

import com.google.gson.*;
import java.nio.file.*;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.world.level.Level;

/** Vanilla bounds and facing used by the native EntityRef movement methods. */
public final class VanillaNativeEntityMovesOracle {
    public static void main(String[] args) throws Exception {
        ParityData.init();
        try (var output = Files.newBufferedWriter(Path.of(args[1]))) {
            for (String line : Files.readAllLines(Path.of(args[0]))) {
                if (line.isBlank()) continue;
                var input = JsonParser.parseString(line).getAsJsonObject();
                var pos = input.getAsJsonArray("pos");
                var point = BlockPos.containing(pos.get(0).getAsDouble(), pos.get(1).getAsDouble(), pos.get(2).getAsDouble());
                var result = new JsonObject();
                result.add("name", input.get("name"));
                result.addProperty("spawnable", Level.isInSpawnableBounds(point));
                // Entity.getDirection widens the entity's float yaw to Direction's double input.
                result.addProperty("direction", Direction.fromYRot((double) input.get("yaw").getAsFloat()).getSerializedName());
                output.write(result.toString());
                output.newLine();
            }
        }
    }
}
