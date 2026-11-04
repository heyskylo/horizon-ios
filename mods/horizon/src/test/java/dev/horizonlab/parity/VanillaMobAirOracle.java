package dev.horizonlab.parity;

import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import net.minecraft.network.syncher.SynchedEntityData;
import net.minecraft.world.entity.Entity;


/** Original oracle: real Entity air/frozen setters and SynchedEntityData spawn/dirty values. */
public final class VanillaMobAirOracle {
    static Field field(Class<?> owner, String name) throws Exception {
        var field = owner.getDeclaredField(name);
        field.setAccessible(true);
        return field;
    }

    static Method definition(Class<?> owner) throws Exception {
        for (var type = owner; type != null; type = type.getSuperclass()) {
            try {
                var method = type.getDeclaredMethod("defineSynchedData", SynchedEntityData.Builder.class);
                method.setAccessible(true);
                return method;
            } catch (NoSuchMethodException ignored) { }
        }
        throw new NoSuchMethodException("defineSynchedData");
    }

    @SuppressWarnings("unchecked")
    static <T> void base(SynchedEntityData.Builder builder, String name, T value) throws Exception {
        builder.define((net.minecraft.network.syncher.EntityDataAccessor<T>) field(Entity.class, name).get(null), value);
    }

    public static void main(String[] args) throws Exception {
        ParityData.init();
        var unsafeClass = Class.forName("sun.misc.Unsafe");
        var unsafe = field(unsafeClass, "theUnsafe").get(null);
        var allocate = unsafeClass.getMethod("allocateInstance", Class.class);
        var entityData = field(Entity.class, "entityData");
        for (var entry : new Object[][]{{"DATA_AIR_SUPPLY_ID", 1}, {"DATA_TICKS_FROZEN", 7}}) {
            var accessor = (net.minecraft.network.syncher.EntityDataAccessor<?>) field(Entity.class, (String) entry[0]).get(null);
            if (accessor.id() != (Integer) entry[1] || accessor.serializer() != net.minecraft.network.syncher.EntityDataSerializers.INT)
                throw new AssertionError("Unexpected pinned metadata contract");
        }
        try (var out = new PrintWriter(Files.newBufferedWriter(Path.of(args[1]), StandardCharsets.UTF_8))) {
            for (String line : Files.readAllLines(Path.of(args[0]))) {
                var parts = line.split(" ");
                String name = switch (parts[0]) {
                    case "cow" -> "net.minecraft.world.entity.animal.cow.Cow";
                    case "cod" -> "net.minecraft.world.entity.animal.fish.Cod";
                    case "dolphin" -> "net.minecraft.world.entity.animal.dolphin.Dolphin";
                    case "axolotl" -> "net.minecraft.world.entity.animal.axolotl.Axolotl";
                    default -> throw new IllegalArgumentException(parts[0]);
                };
                var type = Class.forName(name);
                var entity = (Entity) allocate.invoke(unsafe, type);
                field(Entity.class, "level").set(entity, ParityData.level);
                var builder = new SynchedEntityData.Builder(entity);
                // Entity's constructor defines its base entries before calling the
                // subclass method. Initialize those entries without constructing a level.
                base(builder, "DATA_SHARED_FLAGS_ID", (byte) 0);
                base(builder, "DATA_AIR_SUPPLY_ID", entity.getMaxAirSupply());
                base(builder, "DATA_CUSTOM_NAME", java.util.Optional.empty());
                base(builder, "DATA_CUSTOM_NAME_VISIBLE", false);
                base(builder, "DATA_SILENT", false);
                base(builder, "DATA_NO_GRAVITY", false);
                base(builder, "DATA_POSE", net.minecraft.world.entity.Pose.STANDING);
                base(builder, "DATA_TICKS_FROZEN", 0);
                definition(type).invoke(entity, builder);
                var data = builder.build();
                entityData.set(entity, data);
                var trace = new ArrayList<String>();
                for (int i = 1; i < parts.length; i++) {
                    var pair = parts[i].split(",");
                    entity.setAirSupply(Integer.parseInt(pair[0]));
                    entity.setTicksFrozen(Integer.parseInt(pair[1]));
                    var values = i == 1 ? data.getNonDefaultValues() : data.packDirty();
                    if (i == 1) data.packDirty();
                    var sent = new ArrayList<String>();
                    if (values != null) {
                        for (var value : values) {
                            if (value.id() == 1 || value.id() == 7) sent.add(value.id() + "=" + value.value());
                        }
                    }
                    trace.add((sent.isEmpty() ? "-" : String.join(",", sent)) + ":" + (entity.getAirSupply() + "," + entity.getTicksFrozen()));
                }
                out.println(String.join(" ", trace));
            }
        }
    }
}
