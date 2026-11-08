package dev.horizonlab.parity;

import com.google.gson.Gson;
import java.nio.file.Files;
import java.nio.file.Path;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import net.minecraft.SharedConstants;
import net.minecraft.server.Bootstrap;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.Marker;
import net.minecraft.world.level.ChunkPos;
import net.minecraft.world.level.entity.ChunkEntities;
import net.minecraft.world.level.entity.EntityPersistentStorage;
import net.minecraft.world.level.entity.LevelCallback;
import net.minecraft.world.level.entity.PersistentEntitySectionManager;
import net.minecraft.world.level.entity.Visibility;

/** Actual Entity.discard and persistent lookup removal, with no entity/manager tick. */
public final class VanillaMobDiscardOracle {
    static Object unsafe;
    static Method allocate;
    static void field(Entity entity, String name, Object value) throws Exception {
        var field = Entity.class.getDeclaredField(name);
        field.setAccessible(true);
        field.set(entity, value);
    }
    static Marker marker(int id, UUID uuid) throws Exception {
        // No level constructor/tick: initialize only the ordinary Entity
        // storage queried by the real manager and Entity.discard below.
        var entity = (Marker) allocate.invoke(unsafe, Marker.class);
        field(entity, "id", id);
        field(entity, "uuid", uuid);
        field(entity, "position", new net.minecraft.world.phys.Vec3(0.5, 64, 2.5));
        field(entity, "blockPosition", new net.minecraft.core.BlockPos(0, 64, 2));
        field(entity, "bb", new net.minecraft.world.phys.AABB(0, 64, 2, 1, 65, 3));
        field(entity, "passengers", com.google.common.collect.ImmutableList.of());
        return entity;
    }
    static final class Storage implements EntityPersistentStorage<Entity> {
        final List<Entity> saved = new ArrayList<>();
        public CompletableFuture<ChunkEntities<Entity>> loadEntities(ChunkPos pos) {
            return CompletableFuture.completedFuture(new ChunkEntities<>(pos, List.of()));
        }
        public void storeEntities(ChunkEntities<Entity> chunk) {
            saved.clear();
            chunk.getEntities().forEach(saved::add);
        }
        public void flush(boolean sync) {}
    }
    static final class Callbacks implements LevelCallback<Entity> {
        int destroyed;
        public void onCreated(Entity entity) {}
        public void onDestroyed(Entity entity) { destroyed++; }
        public void onTickingStart(Entity entity) {}
        public void onTickingEnd(Entity entity) {}
        public void onTrackingStart(Entity entity) {}
        public void onTrackingEnd(Entity entity) {}
        public void onSectionChange(Entity entity) {}
    }
    public static void main(String[] args) throws Exception {
        SharedConstants.tryDetectVersion();
        Bootstrap.bootStrap();
        var allocator = Class.forName("sun.misc.Unsafe");
        var singleton = allocator.getDeclaredField("theUnsafe");
        singleton.setAccessible(true);
        unsafe = singleton.get(null);
        allocate = allocator.getMethod("allocateInstance", Class.class);
        var store = new Storage();
        var callbacks = new Callbacks();
        var manager = new PersistentEntitySectionManager<>(Entity.class, callbacks, store);
        var chunk = new ChunkPos(0, 0);
        manager.updateChunkStatus(chunk, Visibility.TICKING);
        manager.processPendingLoads();
        var entity = marker(42, new UUID(1, 2));
        if (!manager.addNewEntity(entity)) throw new AssertionError("add");
        var getter = manager.getEntityGetter();
        var result = new LinkedHashMap<String, Boolean>();
        result.put("before_id", getter.get(42) == entity);
        result.put("before_uuid", getter.get(entity.getUUID()) == entity);
        manager.autoSave();
        result.put("before_save", store.saved.contains(entity));
        entity.discard();
        result.put("after_id", getter.get(42) != null);
        result.put("after_uuid", getter.get(entity.getUUID()) != null);
        result.put("after_enumeration", getter.getAll().iterator().hasNext());
        manager.autoSave();
        result.put("after_save", store.saved.contains(entity));
        result.put("removed", entity.isRemoved());
        if (callbacks.destroyed != 1) throw new AssertionError("destroy callback");
        // A later entity with the same UUID is registered immediately, while
        // the removed entity still exists as a Java reference.
        var replacement = marker(44, entity.getUUID());
        if (!manager.addNewEntity(replacement) || getter.get(entity.getUUID()) != replacement)
            throw new AssertionError("UUID reuse");
        Files.writeString(Path.of(args[0]), new Gson().toJson(result));
    }
}
