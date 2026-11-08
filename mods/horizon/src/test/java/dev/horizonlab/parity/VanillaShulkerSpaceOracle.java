package dev.horizonlab.parity;

import com.google.gson.JsonParser;
import java.io.PrintWriter;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.monster.Shulker;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;

/** Original oracle: the native shulker attachment predicate on a recording level. */
public final class VanillaShulkerSpaceOracle {
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var allocator = Class.forName("sun.misc.Unsafe");
        var field = allocator.getDeclaredField("theUnsafe");
        field.setAccessible(true);
        VanillaBlockUpdateOracle.unsafe = field.get(null);
        VanillaBlockUpdateOracle.allocate = allocator.getMethod("allocateInstance", Class.class);
        VanillaBlockUpdateOracle.AIR = Blocks.AIR.defaultBlockState();
        VanillaBlockUpdateOracle.OVERWORLD_TYPE = VanillaBlockUpdateOracle.loadOverworld();
        var stay = Shulker.class.getDeclaredMethod("canStayAt", BlockPos.class, Direction.class);
        stay.setAccessible(true);
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8))) {
            for (var line : Files.readAllLines(Path.of(args[0]))) {
                var input = JsonParser.parseString(line).getAsJsonObject();
                // Resolve names through actual vanilla states; the shared world takes numeric ids.
                var cells = input.getAsJsonArray("cells");
                for (var cell : cells) {
                    var row = cell.getAsJsonArray();
                    var name = row.get(3).getAsString();
                    var state = net.minecraft.core.registries.BuiltInRegistries.BLOCK.getValue(
                        net.minecraft.resources.Identifier.parse(name)).defaultBlockState();
                    row.set(3, new com.google.gson.JsonPrimitive(Block.getId(state)));
                }
                var world = VanillaBlockUpdateOracle.build(input);
                world.registries = ParityData.access;
                var shulker = new Shulker(EntityTypes.SHULKER, world);
                var current = input.getAsJsonArray("current");
                shulker.setPos(current.get(0).getAsDouble(), current.get(1).getAsDouble(), current.get(2).getAsDouble());
                var target = VanillaBlockUpdateOracle.readPos(input.getAsJsonArray("target"));
                var direction = Direction.from3DDataValue(input.get("face").getAsInt());
                out.println(stay.invoke(shulker, target, direction));
            }
        }
    }
}
