package dev.horizonlab.parity;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonParser;
import com.mojang.serialization.JsonOps;
import java.io.InputStream;
import java.lang.reflect.Method;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import net.minecraft.client.gui.font.providers.GlyphProviderDefinition;
import net.minecraft.client.renderer.texture.atlas.SpriteSourceList;
import net.minecraft.client.renderer.texture.atlas.SpriteSources;
import net.minecraft.client.resources.language.ClientLanguage;
import net.minecraft.client.resources.sounds.Sound;
import net.minecraft.client.sounds.SoundManager;
import net.minecraft.client.sounds.WeighedSoundEvents;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.contents.TranslatableContents;
import net.minecraft.resources.Identifier;
import net.minecraft.server.packs.PackLocationInfo;
import net.minecraft.server.packs.PackResources;
import net.minecraft.server.packs.PackType;
import net.minecraft.server.packs.PathPackResources;
import net.minecraft.server.packs.repository.FolderRepositorySource;
import net.minecraft.server.packs.repository.PackRepository;
import net.minecraft.server.packs.repository.PackSource;
import net.minecraft.server.packs.resources.MultiPackResourceManager;
import net.minecraft.server.packs.resources.Resource;
import net.minecraft.util.profiling.InactiveProfiler;
import net.minecraft.world.level.validation.DirectoryValidator;

/**
 * The client resource manager over vanilla's resources and a selection of
 * resource packs from a folder (FolderRepositorySource: folder and zip packs,
 * pack.mcmeta overlays and filters), and what the reload listeners read from
 * it: which pack provides a file, translations (ClientLanguage), sound events
 * (SoundManager.prepare), font providers (FontManager) and atlas sources
 * (SpriteSourceList).
 *
 * Input: a JSON array ["root", VANILLA, PACKS, ID...] (bottom first), then queries:
 * "file PATH", "lang KEY", "sound EVENT", "font ID", "atlas ID".
 */
public final class VanillaPackResourcesOracle {
    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        SpriteSources.bootstrap();
        List<String> lines = Files.readAllLines(Path.of(args[0]));
        JsonArray root = JsonParser.parseString(lines.get(0)).getAsJsonArray();
        if (root.size() < 3 || !root.get(0).getAsString().equals("root")) {
            throw new IllegalArgumentException("invalid resource-pack header");
        }
        List<PackResources> packs = new ArrayList<>();
        packs.add(new PathPackResources(
            new PackLocationInfo("vanilla", Component.literal("vanilla"), PackSource.BUILT_IN, Optional.empty()), Path.of(root.get(1).getAsString())));
        var repository = new PackRepository(new FolderRepositorySource(
            Path.of(root.get(2).getAsString()), PackType.CLIENT_RESOURCES, PackSource.DEFAULT, new DirectoryValidator(path -> true)));
        repository.reload();
        for (int i = 3; i < root.size(); i++) {
            repository.getPack(root.get(i).getAsString()).open().forEach(packs::add);
        }
        var manager = new MultiPackResourceManager(PackType.CLIENT_RESOURCES, packs);
        ClientLanguage language = ClientLanguage.loadFrom(manager, List.of("en_us"), false);
        Object unsafe = theUnsafe();
        SoundManager sounds = (SoundManager) unsafe.getClass().getMethod("allocateInstance", Class.class)
            .invoke(unsafe, SoundManager.class);
        Method prepare = SoundManager.class.getDeclaredMethod("prepare",
            net.minecraft.server.packs.resources.ResourceManager.class, net.minecraft.util.profiling.ProfilerFiller.class);
        prepare.setAccessible(true);
        Object preparations = prepare.invoke(sounds, manager, InactiveProfiler.INSTANCE);
        var registryField = preparations.getClass().getDeclaredField("registry");
        registryField.setAccessible(true);
        @SuppressWarnings("unchecked")
        Map<Identifier, WeighedSoundEvents> registry = (Map<Identifier, WeighedSoundEvents>) registryField.get(preparations);
        Method fontStack = net.minecraft.client.gui.font.FontManager.class.getDeclaredMethod("loadResourceStack", List.class, Identifier.class);
        fontStack.setAccessible(true);

        StringBuilder out = new StringBuilder();
        for (String line : lines.subList(1, lines.size())) {
            String[] q = line.split(" ", 2);
            out.append(line).append(" =>");
            switch (q[0]) {
                case "file" -> {
                    Optional<Resource> resource = manager.getResource(Identifier.parse(q[1]));
                    if (resource.isEmpty()) {
                        out.append(" missing");
                    } else {
                        try (InputStream in = resource.get().open()) {
                            out.append(' ').append(Long.toHexString(fnv(in.readAllBytes())));
                        }
                    }
                }
                case "lang" -> out.append(' ').append(language.getOrDefault(q[1]));
                case "sound" -> {
                    WeighedSoundEvents event = registry.get(Identifier.parse(q[1]));
                    if (event == null) {
                        out.append(" missing");
                        break;
                    }
                    Component subtitle = event.getSubtitle();
                    out.append(" subtitle=").append(subtitle == null ? "none"
                        : ((TranslatableContents) subtitle.getContents()).getKey());
                    var listField = WeighedSoundEvents.class.getDeclaredField("list");
                    listField.setAccessible(true);
                    for (Object weighted : (List<?>) listField.get(event)) {
                        out.append(' ').append(weighted instanceof Sound sound
                            ? sound.getLocation() + "*" + sound.getWeight() : "event");
                    }
                }
                case "font" -> {
                    Identifier id = Identifier.parse(q[1]);
                    List<Resource> stack = manager.getResourceStack(id.withPath(p -> "font/" + p + ".json"));
                    List<?> builders = new ArrayList<>((List<?>) fontStack.invoke(null, stack, id));
                    java.util.Collections.reverse(builders);
                    for (Object pair : builders) {
                        var conditional = (GlyphProviderDefinition.Conditional) ((com.mojang.datafixers.util.Pair<?, ?>) pair).getSecond();
                        JsonElement json = GlyphProviderDefinition.Conditional.CODEC.encodeStart(JsonOps.INSTANCE, conditional).getOrThrow();
                        out.append(' ').append(strip(json.getAsJsonObject().get("type").getAsString()));
                    }
                }
                case "atlas" -> {
                    SpriteSourceList list = SpriteSourceList.load(manager, Identifier.parse(q[1]));
                    var sourcesField = SpriteSourceList.class.getDeclaredField("sources");
                    sourcesField.setAccessible(true);
                    @SuppressWarnings("unchecked")
                    var sources = (List<net.minecraft.client.renderer.texture.atlas.SpriteSource>) sourcesField.get(list);
                    JsonArray json = SpriteSources.FILE_CODEC.encodeStart(JsonOps.INSTANCE, sources).getOrThrow()
                        .getAsJsonObject().getAsJsonArray("sources");
                    for (JsonElement source : json) {
                        out.append(' ').append(strip(source.getAsJsonObject().get("type").getAsString()));
                    }
                }
                default -> throw new IllegalArgumentException(line);
            }
            out.append('\n');
        }
        Files.writeString(Path.of(args[1]), out);
    }

    private static String strip(String type) {
        return type.startsWith("minecraft:") ? type.substring("minecraft:".length()) : type;
    }

    /** FNV-1a 64 of a file's bytes. */
    private static long fnv(byte[] bytes) {
        long hash = 0xcbf29ce484222325L;
        for (byte b : bytes) {
            hash = (hash ^ (b & 0xff)) * 0x100000001b3L;
        }
        return hash;
    }

    private static Object theUnsafe() throws Exception {
        Class<?> unsafeClass = Class.forName("sun.misc.Unsafe");
        var field = unsafeClass.getDeclaredField("theUnsafe");
        field.setAccessible(true);
        return field.get(null);
    }
}
