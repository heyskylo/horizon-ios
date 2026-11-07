package dev.horizonlab.parity;

import com.google.gson.*;
import java.io.PrintWriter;
import java.nio.file.*;
import java.util.List;
import net.minecraft.core.BlockPos;
import net.minecraft.core.component.DataComponents;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.component.BrewingFuel;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.BrewingStandBlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.storage.loot.LootContext;

/** Calls native stack lookup, slot predicates, provider records and serverTick without a server. */
public final class VanillaBrewingFuelOracle {
    static LootContext context;
    static final class World extends ServerLevel {
        World() { super(null, null, null, null, null, null, false, 0L, List.of(), false); }
        @Override public void blockEntityChanged(BlockPos pos) {}
        @Override public void updateNeighbourForOutputSignal(BlockPos pos, Block block) {}
    }
    static final class Stand extends BrewingStandBlockEntity {
        Stand() { super(BlockPos.ZERO, Blocks.BREWING_STAND.defaultBlockState()); }
        // Substitute only context creation, retaining native provider evaluation.
        @Override protected int getUses(ServerLevel level, BrewingFuel fuel) { return fuel.uses().get(context, 0); }
        @Override protected float getSpeedMultiplier(ServerLevel level, BrewingFuel fuel) { return fuel.speedMultiplier().get(context, 1); }
    }
    static Object field(Object object, String name) throws Exception {
        var field = BrewingStandBlockEntity.class.getDeclaredField(name);
        field.setAccessible(true); return field.get(object);
    }
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var constructor = LootContext.class.getDeclaredConstructors()[0];
        constructor.setAccessible(true);
        context = (LootContext) constructor.newInstance(null, net.minecraft.util.RandomSource.create(0), ParityData.lookup);
        var unsafeClass = Class.forName("sun.misc.Unsafe");
        var unsafeField = unsafeClass.getDeclaredField("theUnsafe"); unsafeField.setAccessible(true);
        var world = (World) unsafeClass.getMethod("allocateInstance", Class.class).invoke(unsafeField.get(null), World.class);
        var slot = Class.forName("net.minecraft.world.inventory.BrewingStandMenu$FuelSlot").getDeclaredMethod("mayPlaceItem", ItemStack.class);
        slot.setAccessible(true);
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1])))) {
            for (String line : Files.readAllLines(Path.of(args[0]))) {
                var input = JsonParser.parseString(line).getAsJsonObject();
                var stack = new ItemStack(BuiltInRegistries.ITEM.getValue(Identifier.parse(input.get("item").getAsString())), input.get("count").getAsInt());
                var patch = input.getAsJsonObject("components");
                if (patch.has("minecraft:brewing_fuel")) stack.set(DataComponents.BREWING_FUEL, BrewingFuel.CODEC.parse(com.mojang.serialization.JsonOps.INSTANCE, patch.get("minecraft:brewing_fuel")).getOrThrow());
                if (patch.has("!minecraft:brewing_fuel")) stack.remove(DataComponents.BREWING_FUEL);
                var stand = new Stand();
                boolean accepted = stand.canPlaceItem(4, stack);
                if (accepted != (boolean) slot.invoke(null, stack) || accepted != stand.canPlaceItemThroughFace(4, stack, net.minecraft.core.Direction.NORTH)) throw new AssertionError("fuel acceptance diverged");
                stand.setItem(4, stack);
                var bottles = BrewingStandBlockEntity.class.getDeclaredField("lastPotionCount"); bottles.setAccessible(true); bottles.set(stand, new boolean[3]);
                for (int tick = 0; tick < 2; tick++) {
                    BrewingStandBlockEntity.serverTick(world, BlockPos.ZERO, Blocks.BREWING_STAND.defaultBlockState(), stand);
                    out.println(accepted + " " + field(stand, "fuel") + " " + field(stand, "totalFuel") + " " + Integer.toUnsignedString(Float.floatToIntBits((float)field(stand, "speedMultiplier"))) + " " + stand.getItem(4).getCount());
                }
            }
        }
    }
}
