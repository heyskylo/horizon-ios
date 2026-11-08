package dev.horizonlab.parity;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.world.entity.EntityTypes;
import net.minecraft.world.entity.ai.attributes.Attributes;
import net.minecraft.world.entity.monster.Shulker;
import net.minecraft.world.entity.projectile.ShulkerBullet;
import net.minecraft.world.level.block.Blocks;

/**
 * Original oracle: where the real ShulkerBullet constructor puts a shulker's bullet, and the
 * Level.loadedAndEntityCanStandOn test that turns a bullet in flight, on a recording level.
 */
public final class VanillaShulkerBulletOracle {
    static Method attachFace;
    static Field peek;

    public static void main(String[] args) throws Exception {
        ParityData.init();
        var allocator = Class.forName("sun.misc.Unsafe");
        var field = allocator.getDeclaredField("theUnsafe");
        field.setAccessible(true);
        VanillaBlockUpdateOracle.unsafe = field.get(null);
        VanillaBlockUpdateOracle.allocate = allocator.getMethod("allocateInstance", Class.class);
        VanillaBlockUpdateOracle.AIR = Blocks.AIR.defaultBlockState();
        VanillaBlockUpdateOracle.OVERWORLD_TYPE = VanillaBlockUpdateOracle.loadOverworld();
        attachFace = Shulker.class.getDeclaredMethod("setAttachFace", Direction.class);
        attachFace.setAccessible(true);
        peek = Shulker.class.getDeclaredField("currentPeekAmount");
        peek.setAccessible(true);
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8))) {
            for (var line : Files.readAllLines(Path.of(args[0]))) {
                var input = JsonParser.parseString(line).getAsJsonObject();
                var world = VanillaBlockUpdateOracle.build(input);
                world.registries = ParityData.access;
                out.println(input.get("mode").getAsString().equals("origin") ? origin(world, input) : stand(world, input));
            }
        }
    }

    /** A shulker with an attach face, lid peek and scale fires at another shulker: the bullet's position. */
    static String origin(VanillaBlockUpdateOracle.World world, JsonObject input) throws Exception {
        var shulker = new Shulker(EntityTypes.SHULKER, world);
        shulker.getAttribute(Attributes.SCALE).setBaseValue(input.get("scale").getAsDouble());
        attachFace.invoke(shulker, Direction.from3DDataValue(input.get("face").getAsInt()));
        peek.setFloat(shulker, Float.intBitsToFloat(input.get("peek").getAsInt()));
        // setPos rebuilds the box (makeBoundingBox) with the peek just set.
        var at = input.getAsJsonArray("position");
        shulker.setPos(at.get(0).getAsDouble(), at.get(1).getAsDouble(), at.get(2).getAsDouble());
        var target = new Shulker(EntityTypes.SHULKER, world);
        var aim = input.getAsJsonArray("target");
        target.setPos(aim.get(0).getAsDouble(), aim.get(1).getAsDouble(), aim.get(2).getAsDouble());
        var bullet = new ShulkerBullet(world, shulker, target, shulker.getAttachFace().getAxis());
        return String.format("%016x %016x %016x", Double.doubleToRawLongBits(bullet.getX()),
            Double.doubleToRawLongBits(bullet.getY()), Double.doubleToRawLongBits(bullet.getZ()));
    }

    /** One block state at `pos` and a bullet with its feet at `y` above the block's centre. */
    static String stand(VanillaBlockUpdateOracle.World world, JsonObject input) throws Exception {
        BlockPos pos = VanillaBlockUpdateOracle.readPos(input.getAsJsonArray("pos"));
        world.cells.put(pos, BlockStateParser.parseForBlock(BuiltInRegistries.BLOCK, input.get("block").getAsString(), false).blockState());
        var bullet = new ShulkerBullet(EntityTypes.SHULKER_BULLET, world);
        bullet.setPos(pos.getX() + 0.5, input.get("y").getAsDouble(), pos.getZ() + 0.5);
        return Boolean.toString(world.loadedAndEntityCanStandOn(pos, bullet));
    }
}
