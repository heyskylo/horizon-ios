package dev.horizonlab.parity;

import com.google.gson.GsonBuilder;
import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.*;
import net.minecraft.client.resources.model.EquipmentAssetManager;
import net.minecraft.client.resources.model.EquipmentClientInfo;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.server.packs.*;
import net.minecraft.server.packs.repository.PackSource;
import net.minecraft.server.packs.resources.*;
import net.minecraft.util.profiling.InactiveProfiler;
import net.minecraft.util.profiling.ProfilerFiller;
import javax.imageio.ImageIO;

/** Actual folder-pack resource enumeration, JSON decoding and EquipmentAssetManager replacement. */
public final class VanillaEquipmentAssetsOracle {
    private static PathPackResources pack(Path path, String id) {
        return new PathPackResources(new PackLocationInfo(id, Component.literal(id), PackSource.DEFAULT, Optional.empty()), path);
    }

    @SuppressWarnings("unchecked")
    public static void main(String[] args) throws Exception {
        Path root = Path.of(args[0]);
        Method prepare = SimpleJsonResourceReloadListener.class.getDeclaredMethod("prepare", ResourceManager.class, ProfilerFiller.class);
        Method apply = EquipmentAssetManager.class.getDeclaredMethod("apply", Map.class, ResourceManager.class, ProfilerFiller.class);
        Field entries = EquipmentAssetManager.class.getDeclaredField("equipmentAssets");
        prepare.setAccessible(true); apply.setAccessible(true); entries.setAccessible(true);
        EquipmentAssetManager manager = new EquipmentAssetManager();
        List<Map<String,Object>> rows = new ArrayList<>();
        for (int phase = 0; phase < 3; phase++) {
            List<PackResources> packs = new ArrayList<>();
            packs.add(pack(root.resolve("base"), "base"));
            if (phase == 1) packs.add(pack(root.resolve("overlay"), "overlay"));
            try (MultiPackResourceManager resources = new MultiPackResourceManager(PackType.CLIENT_RESOURCES, packs)) {
                Map<Identifier,EquipmentClientInfo> prepared = (Map<Identifier,EquipmentClientInfo>) prepare.invoke(manager, resources, InactiveProfiler.INSTANCE);
                apply.invoke(manager, prepared, resources, InactiveProfiler.INSTANCE);
                Map<ResourceKey<?>,EquipmentClientInfo> loaded = (Map<ResourceKey<?>,EquipmentClientInfo>) entries.get(manager);
                for (var entry : loaded.entrySet()) {
                    for (var kind : new EquipmentClientInfo.LayerType[] {EquipmentClientInfo.LayerType.HUMANOID, EquipmentClientInfo.LayerType.HUMANOID_LEGGINGS, EquipmentClientInfo.LayerType.WINGS}) {
                        List<EquipmentClientInfo.Layer> layers = entry.getValue().getLayers(kind);
                        for (int index = 0; index < layers.size(); index++) {
                            var layer = layers.get(index);
                            Identifier texture = layer.getTextureLocation(kind);
                            String path = texture.getPath();
                            path = path.substring("textures/".length(), path.length() - ".png".length());
                            String sprite = texture.getNamespace().equals("minecraft") ? path : texture.getNamespace() + ":" + path;
                            var row = new LinkedHashMap<String,Object>();
                            row.put("phase",phase); row.put("id",entry.getKey().identifier().toString());
                            row.put("kind",kind.getSerializedName()); row.put("index",index); row.put("texture",sprite);
                            row.put("playerTexture",layer.usePlayerTexture()); row.put("dyeable",layer.dyeable().isPresent());
                            row.put("undyed",layer.dyeable().flatMap(EquipmentClientInfo.Dyeable::colorWhenUndyed).orElse(null));
                            try (var input = resources.getResourceOrThrow(texture).open()) {
                                int pixel = ImageIO.read(input).getRGB(0,0);
                                row.put("pixel", List.of((pixel >>> 16) & 255, (pixel >>> 8) & 255, pixel & 255, (pixel >>> 24) & 255));
                            }
                            rows.add(row);
                        }
                    }
                }
            }
        }
        rows.sort(Comparator.comparing((Map<String,Object> r) -> (Integer)r.get("phase"))
            .thenComparing(r -> (String)r.get("id")).thenComparing(r -> (String)r.get("kind"))
            .thenComparing(r -> (Integer)r.get("index")));
        Files.writeString(Path.of(args[1]), new GsonBuilder().serializeNulls().create().toJson(rows));
    }
}
