package dev.horizonlab.parity;

import com.google.gson.Gson;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.LinkedHashMap;
import java.util.Optional;
import net.minecraft.client.resources.model.EquipmentClientInfo;
import net.minecraft.resources.Identifier;

/** Calls the pinned renderer's actual color decision without constructing a game or graphics context. */
public final class VanillaEquipmentColorOracle {
    public static void main(String[] args) throws Exception {
        Method color = Class.forName("net.minecraft.client.renderer.entity.layers.EquipmentLayerRenderer")
            .getDeclaredMethod("getColorForLayer", EquipmentClientInfo.Layer.class, int.class);
        color.setAccessible(true);
        String[] modes = {"plain", "only_dyed", "black", "brown", "white"};
        int[] colors = {0, 0, 0, 0xA06540, 0xFFFFFF};
        int[] dyed = {0, 0xFF000000, 0xFF123456};
        StringBuilder lines = new StringBuilder();
        Gson gson = new Gson();
        for (int i = 0; i < modes.length; i++) {
            Optional<EquipmentClientInfo.Dyeable> dye = i == 0 ? Optional.empty()
                : Optional.of(new EquipmentClientInfo.Dyeable(i == 1 ? Optional.empty() : Optional.of(colors[i])));
            var layer = new EquipmentClientInfo.Layer(Identifier.withDefaultNamespace("fixture"), dye, false);
            for (int stackColor : dyed) {
                int argb = (int) color.invoke(null, layer, stackColor);
                int abgr = (argb & 0xFF00FF00) | ((argb >>> 16) & 0xFF) | ((argb & 0xFF) << 16);
                var row = new LinkedHashMap<String, Object>();
                row.put("mode", modes[i]);
                row.put("dyed", Integer.toUnsignedLong(stackColor));
                row.put("color", Integer.toUnsignedLong(abgr));
                row.put("draw", argb != 0);
                lines.append(gson.toJson(row)).append('\n');
            }
        }
        Files.writeString(Path.of(args[0]), lines);
    }
}
