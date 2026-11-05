package dev.horizonlab.parity;

import com.google.common.cache.LoadingCache;
import com.google.gson.GsonBuilder;
import com.mojang.authlib.GameProfile;
import com.mojang.authlib.SignatureState;
import com.mojang.authlib.minecraft.*;
import com.mojang.authlib.properties.Property;
import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.*;
import net.minecraft.client.renderer.entity.state.*;
import net.minecraft.client.renderer.texture.SkinTextureDownloader;
import net.minecraft.client.resources.SkinManager;
import net.minecraft.core.ClientAsset;
import net.minecraft.resources.Identifier;
import net.minecraft.server.Services;
import net.minecraft.world.entity.player.PlayerSkin;
import sun.misc.Unsafe;

/**
 * Runs the real SkinManager.registerTextures, cache completion handler,
 * createLookup and WingsLayer.getPlayerElytraTexture. Only the download endpoint
 * is controlled: ready, exceptional or incomplete futures. No HTTP or graphics
 * context is started; this does not test network timeout/cache expiry policy.
 */
public final class VanillaPlayerSkinOracle {
    private static String texture(ClientAsset.Texture texture) {
        if (texture == null) return null;
        String path = texture.texturePath().getPath();
        return path.startsWith("textures/") && path.endsWith(".png")
            ? path.substring(9, path.length() - 4) : path;
    }

    @SuppressWarnings("unchecked")
    public static void main(String[] args) throws Exception {
        UUID id = UUID.fromString("01234567-89ab-cdef-0123-456789abcdef");
        Property property = new Property("textures", "controlled-endpoint", "valid");
        SessionService session = (SessionService) Proxy.newProxyInstance(
            SessionService.class.getClassLoader(), new Class<?>[] {SessionService.class},
            (proxy, method, values) -> {
                if (method.getName().equals("getPackedTextures")) return property;
                throw new UnsupportedOperationException(method.getName());
            });
        Services services = new Services(session, null, null, null, null);
        Method register = SkinManager.class.getDeclaredMethod("registerTextures", UUID.class, MinecraftProfileTextures.class);
        register.setAccessible(true);
        Class<?> keyType = Class.forName("net.minecraft.client.resources.SkinManager$CacheKey");
        Constructor<?> keyConstructor = keyType.getDeclaredConstructor(UUID.class, Property.class);
        keyConstructor.setAccessible(true);
        Object key = keyConstructor.newInstance(id, property);
        Method reduce = Class.forName("net.minecraft.client.resources.SkinManager$1")
            .getDeclaredMethod("lambda$load$2", keyType, PlayerSkin.class, Throwable.class);
        reduce.setAccessible(true);
        Field cacheField = SkinManager.class.getDeclaredField("skinCache");
        cacheField.setAccessible(true);
        Field unsafeField = Unsafe.class.getDeclaredField("theUnsafe");
        unsafeField.setAccessible(true);
        Unsafe unsafe = (Unsafe) unsafeField.get(null);
        Method wings = Class.forName("net.minecraft.client.renderer.entity.layers.WingsLayer")
            .getDeclaredMethod("getPlayerElytraTexture", HumanoidRenderState.class);
        wings.setAccessible(true);
        var gson = new GsonBuilder().serializeNulls().create();
        var output = new StringBuilder();
        for (boolean skin : new boolean[] {false, true}) {
            for (boolean cape : new boolean[] {false, true}) {
                for (boolean elytra : new boolean[] {false, true}) {
                    for (String state : elytra ? new String[] {"ready", "failed", "pending"} : new String[] {"ready"}) {
                        var submitted = new ArrayList<String>();
                        SkinTextureDownloader downloader = new SkinTextureDownloader(java.net.Proxy.NO_PROXY, null, Runnable::run) {
                            @Override
                            public CompletableFuture<ClientAsset.Texture> downloadAndRegisterSkin(Identifier identifier, Path file, String url, boolean legacy) {
                                submitted.add(url.substring(url.lastIndexOf('/') + 1));
                                if (url.endsWith("/elytra")) {
                                    if (state.equals("failed")) return CompletableFuture.failedFuture(new java.io.IOException("intentional fixture failure"));
                                    if (state.equals("pending")) return new CompletableFuture<>();
                                }
                                return CompletableFuture.completedFuture(new ClientAsset.DownloadedTexture(identifier, url));
                            }
                        };
                        SkinManager manager = new SkinManager(Path.of(args[0]), services, downloader, Runnable::run);
                        MinecraftProfileTextures textures = new MinecraftProfileTextures(
                            skin ? new MinecraftProfileTexture("https://textures.minecraft.net/texture/body", Map.of()) : null,
                            cape ? new MinecraftProfileTexture("https://textures.minecraft.net/texture/cape", Map.of()) : null,
                            elytra ? new MinecraftProfileTexture("https://textures.minecraft.net/texture/elytra", Map.of()) : null,
                            SignatureState.SIGNED);
                        CompletableFuture<PlayerSkin> registered = (CompletableFuture<PlayerSkin>) register.invoke(manager, id, textures);
                        CompletableFuture<Optional<PlayerSkin>> finished = registered.handle((value, error) -> {
                            try {
                                return (Optional<PlayerSkin>) reduce.invoke(null, key, value, error);
                            } catch (ReflectiveOperationException e) {
                                throw new RuntimeException(e);
                            }
                        });
                        var cache = (LoadingCache<Object, CompletableFuture<Optional<PlayerSkin>>>) cacheField.get(manager);
                        cache.put(key, finished);
                        PlayerSkin result = manager.createLookup(new GameProfile(id, "Fixture"), true).get();
                        for (boolean showCape : new boolean[] {false, true}) {
                            AvatarRenderState avatar = (AvatarRenderState) unsafe.allocateInstance(AvatarRenderState.class);
                            avatar.skin = result;
                            avatar.showCape = showCape;
                            Identifier wing = (Identifier) wings.invoke(null, avatar);
                            var row = new LinkedHashMap<String, Object>();
                            row.put("skin", skin); row.put("cape", cape); row.put("elytra", elytra);
                            row.put("state", state); row.put("showCape", showCape);
                            row.put("submitted", submitted);
                            row.put("body", texture(result.body()));
                            row.put("capeTexture", texture(result.cape()));
                            row.put("elytraTexture", texture(result.elytra()));
                            row.put("wing", wing == null ? null : wing.getPath());
                            output.append(gson.toJson(row)).append('\n');
                        }
                    }
                }
            }
        }
        Files.writeString(Path.of(args[1]), output);
    }
}
