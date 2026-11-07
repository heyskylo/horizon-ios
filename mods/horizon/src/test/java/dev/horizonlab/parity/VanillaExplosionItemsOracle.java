package dev.horizonlab.parity;

import com.google.gson.*;
import java.lang.reflect.Field;
import java.nio.file.*;
import java.util.*;
import net.minecraft.core.RegistryAccess;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.damagesource.DamageSources;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.item.ItemEntity;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.gamerules.GameRules;
import net.minecraft.world.phys.Vec3;

/** Real ItemEntity.hurtServer, including fractional health and onDestroyed.
 * A stand-in level records spawned contents; no server is started. Distances
 * describe radius-one blasts in empty air (full exposure). Blast geometry is
 * controlled here; the Rust fixture runs its actual explosion entity walk.
 */
public final class VanillaExplosionItemsOracle {
    static final class Lvl extends ServerLevel {
        int next;
        GameRules rules;
        List<ItemEntity> items;
        Lvl() { super(null, null, null, null, null, null, false, 0L, List.of(), false); }
        @Override public int getNextEntityId() { return next++; }
        @Override public RegistryAccess registryAccess() { return ParityData.access; }
        @Override public GameRules getGameRules() { return rules; }
        @Override public boolean addFreshEntity(Entity entity) {
            if (entity instanceof ItemEntity item) items.add(item);
            return true;
        }
        @Override public void gameEvent(net.minecraft.core.Holder<net.minecraft.world.level.gameevent.GameEvent> event, Vec3 at, net.minecraft.world.level.gameevent.GameEvent.Context context) {}
    }

    public static void main(String[] args) throws Exception {
        ParityData.init();
        VanillaVehicleContainerOracle.reflect();
        Lvl level = (Lvl) VanillaVehicleContainerOracle.allocate.invoke(VanillaVehicleContainerOracle.unsafe, Lvl.class);
        level.rules = new GameRules(net.minecraft.world.flag.FeatureFlags.REGISTRY.allFlags());
        Field registry = Level.class.getDeclaredField("registryAccess");
        long offset = (long) VanillaVehicleContainerOracle.objectFieldOffset.invoke(VanillaVehicleContainerOracle.unsafe, registry);
        VanillaVehicleContainerOracle.putObject.invoke(VanillaVehicleContainerOracle.unsafe, level, offset, ParityData.access);
        Field health = ItemEntity.class.getDeclaredField("health"); health.setAccessible(true);
        var source = new DamageSources(ParityData.access).explosion(null, null);
        JsonArray output = new JsonArray();
        for (var value : JsonParser.parseString(Files.readString(Path.of(args[0]))).getAsJsonArray()) {
            JsonObject c = value.getAsJsonObject();
            level.items = new ArrayList<>();
            ItemEntity original = new ItemEntity(level, 0, 100, 0, ParityData.stack(c.get("stack").getAsString()));
            health.setInt(original, c.get("health").getAsInt());
            level.items.add(original);
            if (c.has("sibling") && c.get("sibling").getAsBoolean()) level.items.add(new ItemEntity(level, 0, 100, 0, new ItemStack(Items.DIRT)));
            JsonArray steps = new JsonArray();
            for (var step : c.getAsJsonArray("distances")) {
                double distance = step.getAsDouble();
                double power = 1.0 - distance / 2.0;
                float damage = (float)((power * power + power) / 2.0 * 14.0 + 1.0);
                // Snapshot before hurting: contents created by onDestroyed are
                // not targets of the explosion that released them.
                for (ItemEntity item : List.copyOf(level.items)) {
                    item.setPos(distance, 100, 0);
                    if (distance <= 2.0) item.hurtServer(level, source, damage);
                }
                level.items.removeIf(Entity::isRemoved);
                List<String> entries = new ArrayList<>();
                for (ItemEntity item : level.items) entries.add("[\"" + net.minecraft.core.registries.BuiltInRegistries.ITEM.getKey(item.getItem().getItem()) + "\"," + item.getItem().getCount() + "," + health.getInt(item) + "]");
                Collections.sort(entries);
                steps.add(JsonParser.parseString("[" + String.join(",", entries) + "]"));
                if (original.isRemoved()) break;
            }
            JsonObject result = new JsonObject(); result.add("name", c.get("name")); result.add("steps", steps); output.add(result);
        }
        Files.writeString(Path.of(args[1]), output.toString());
    }
}
