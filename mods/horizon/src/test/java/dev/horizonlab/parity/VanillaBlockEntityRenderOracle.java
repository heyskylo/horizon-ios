package dev.horizonlab.parity;

import com.google.gson.*;
import com.mojang.blaze3d.vertex.PoseStack;
import com.mojang.blaze3d.vertex.VertexConsumer;
import java.lang.reflect.*;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.*;
import net.minecraft.client.gui.Font;
import net.minecraft.client.model.Model;
import net.minecraft.client.model.geom.EntityModelSet;
import net.minecraft.client.model.geom.LayerDefinitions;
import net.minecraft.client.model.object.banner.BannerFlagModel;
import net.minecraft.client.renderer.Sheets;
import net.minecraft.client.renderer.texture.OverlayTexture;
import net.minecraft.client.renderer.SubmitNodeCollector;
import net.minecraft.client.renderer.blockentity.*;
import net.minecraft.client.renderer.blockentity.state.*;
import net.minecraft.client.renderer.rendertype.RenderType;
import net.minecraft.client.renderer.state.level.CameraRenderState;
import net.minecraft.client.renderer.texture.TextureAtlasSprite;
import net.minecraft.client.renderer.texture.UvMapping;
import net.minecraft.client.resources.model.sprite.SpriteGetter;
import net.minecraft.client.resources.model.sprite.SpriteId;
import net.minecraft.commands.arguments.blocks.BlockStateParser;
import net.minecraft.core.BlockPos;
import net.minecraft.core.Direction;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.core.registries.Registries;
import net.minecraft.network.chat.Component;
import net.minecraft.network.chat.ComponentSerialization;
import com.mojang.serialization.JsonOps;
import net.minecraft.resources.Identifier;
import net.minecraft.resources.ResourceKey;
import net.minecraft.util.FormattedCharSequence;
import net.minecraft.world.item.DyeColor;
import net.minecraft.world.item.ItemStackTemplate;
import net.minecraft.world.level.block.EntityBlock;
import net.minecraft.world.level.block.entity.*;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.ChestType;
import net.minecraft.world.phys.Vec3;
import org.joml.Matrix4f;
import org.joml.Quaternionf;

/**
 * Original oracle for block-entity rendering: each case builds a vanilla block
 * entity for a block state, sets its animation fields, runs the real
 * renderer's extractRenderState and submit, and captures every submitted
 * model (vertices after setupAnim and the pose stack, sprite-local UVs, tint,
 * light), text call and item. Level-backed inputs (light, chest lid
 * openness, pot wobble) come from the case because the entity has no level;
 * End gateways, which query theirs, get a stub level built from the case.
 *
 * Input lines: case JSON. Output lines: {case, submits:[...]} or {case, error}.
 */
public final class VanillaBlockEntityRenderOracle {
    private static final Map<TextureAtlasSprite, SpriteId> SPRITES = new IdentityHashMap<>();
    private static Font font;
    private static Object unsafe;
    private static Method allocate;

    static String textureName(Identifier id) {
        String path = id.getPath();
        if (path.startsWith("textures/")) path = path.substring(9);
        if (path.endsWith(".png")) path = path.substring(0, path.length() - 4);
        return id.getNamespace().equals("minecraft") ? path : id.getNamespace() + ":" + path;
    }

    @SuppressWarnings("unchecked")
    static Identifier renderTypeTexture(RenderType type) throws Exception {
        Field state = RenderType.class.getDeclaredField("state");
        state.setAccessible(true);
        Object setup = state.get(type);
        Field textures = setup.getClass().getDeclaredField("textures");
        textures.setAccessible(true);
        var map = (Map<String, Object>) textures.get(setup);
        Object binding = map.get("Sampler0");
        if (binding == null) return null;
        Method location = binding.getClass().getDeclaredMethod("location");
        location.setAccessible(true);
        return (Identifier) location.invoke(binding);
    }

    static JsonArray matrix(Matrix4f m) {
        var a = new JsonArray();
        float[] f = new float[16];
        m.get(f);
        for (float x : f) a.add(x);
        return a;
    }

    /** A vertex consumer recording [x,y,z,color,u,v,overlay,light,nx,ny,nz]. */
    static VertexConsumer recorder(List<float[]> out) {
        InvocationHandler handler = new InvocationHandler() {
            @Override
            public Object invoke(Object proxy, Method method, Object[] args) throws Throwable {
                if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, args);
                float[] v = out.isEmpty() ? null : out.get(out.size() - 1);
                switch (method.getName()) {
                    case "addVertex" -> out.add(new float[] {(float) args[0], (float) args[1], (float) args[2], 0, 0, 0, 0, 0, 0, 0, 0});
                    case "setColor" -> v[3] = Float.intBitsToFloat(args.length == 1 ? (int) args[0]
                        : ((int) args[3] << 24) | ((int) args[0] << 16) | ((int) args[1] << 8) | (int) args[2]);
                    case "setUv" -> { v[4] = (float) args[0]; v[5] = (float) args[1]; }
                    case "setUv1" -> v[6] = Float.intBitsToFloat(((int) args[0]) | ((int) args[1] << 16));
                    case "setUv2" -> v[7] = Float.intBitsToFloat(((int) args[0]) | ((int) args[1] << 16));
                    case "setNormal" -> { v[8] = (float) args[0]; v[9] = (float) args[1]; v[10] = (float) args[2]; }
                    default -> { }
                }
                return proxy;
            }
        };
        return (VertexConsumer) Proxy.newProxyInstance(VertexConsumer.class.getClassLoader(), new Class<?>[] {VertexConsumer.class}, handler);
    }

    static JsonArray vertices(List<float[]> verts) {
        var out = new JsonArray();
        for (float[] v : verts) {
            var a = new JsonArray();
            a.add(v[0]); a.add(v[1]); a.add(v[2]);
            a.add(Float.floatToRawIntBits(v[3]));
            a.add(v[4]); a.add(v[5]);
            a.add(Float.floatToRawIntBits(v[6])); a.add(Float.floatToRawIntBits(v[7]));
            a.add(v[8]); a.add(v[9]); a.add(v[10]);
            out.add(a);
        }
        return out;
    }

    static String text(FormattedCharSequence sequence) {
        var sb = new StringBuilder();
        sequence.accept((index, style, codepoint) -> {
            sb.appendCodePoint(codepoint);
            return true;
        });
        return sb.toString();
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    static SubmitNodeCollector collector(JsonArray submits) {
        InvocationHandler handler = new InvocationHandler() {
            @Override
            public Object invoke(Object proxy, Method method, Object[] args) throws Throwable {
                String name = method.getName();
                if (name.equals("order")) return proxy;
                if (method.isDefault()) return InvocationHandler.invokeDefault(proxy, method, args);
                var submit = new JsonObject();
                submit.addProperty("type", name);
                switch (name) {
                    case "submitModel" -> {
                        var model = (Model) args[0];
                        var pose = (PoseStack) args[2];
                        var renderType = (RenderType) args[3];
                        var uv = (UvMapping) args[7];
                        String texture;
                        if (uv instanceof TextureAtlasSprite sprite && SPRITES.containsKey(sprite))
                            texture = textureName(SPRITES.get(sprite).texture());
                        else {
                            Identifier t = renderTypeTexture(renderType);
                            texture = t == null ? "?" : textureName(t);
                        }
                        submit.addProperty("texture", texture);
                        submit.addProperty("renderType", renderType.toString().split("\\[")[0]);
                        submit.addProperty("light", (int) args[4]);
                        submit.addProperty("overlay", (int) args[5]);
                        submit.addProperty("tint", (int) args[6]);
                        model.setupAnim(args[1]);
                        List<float[]> verts = new ArrayList<>();
                        model.renderToBuffer(pose, recorder(verts), (int) args[4], (int) args[5], (int) args[6]);
                        submit.add("vertices", vertices(verts));
                    }
                    case "submitText" -> {
                        var pose = (PoseStack) args[0];
                        submit.add("matrix", matrix(pose.last().pose()));
                        submit.addProperty("x", (float) args[1]);
                        submit.addProperty("y", (float) args[2]);
                        submit.addProperty("text", text((FormattedCharSequence) args[3]));
                        submit.addProperty("shadow", (boolean) args[4]);
                        submit.addProperty("mode", args[5].toString());
                        submit.addProperty("light", (int) args[6]);
                        submit.addProperty("color", (int) args[7]);
                        submit.addProperty("background", (int) args[8]);
                        submit.addProperty("outline", (int) args[9]);
                        var sequence = (FormattedCharSequence) args[3];
                        submit.add("glyphs", glyphs(sequence, (float) args[1], (float) args[2], (int) args[7], (int) args[6], pose.last().pose()));
                        boolean[] obfuscated = {false};
                        sequence.accept((index, style, c) -> { obfuscated[0] |= style.isObfuscated(); return true; });
                        if (!obfuscated[0]) submit.add("textVertices", textVertices(sequence, (float) args[1], (float) args[2], (int) args[7] | 0xff000000, (int) args[9] == 0 ? 0 : (int) args[9] | 0xff000000, (int) args[6]));
                    }
                    case "submitCustomGeometry" -> {
                        var pose = (PoseStack) args[0];
                        var renderType = (RenderType) args[1];
                        Identifier t = renderTypeTexture(renderType);
                        submit.addProperty("texture", t == null ? "?" : textureName(t));
                        submit.addProperty("renderType", renderType.toString().split("\\[")[0]);
                        List<float[]> verts = new ArrayList<>();
                        ((SubmitNodeCollector.CustomGeometryRenderer) args[2]).render(pose.last(), recorder(verts));
                        submit.add("vertices", vertices(verts));
                    }
                    case "submitItem" -> {
                        var pose = (PoseStack) args[0];
                        submit.add("matrix", matrix(pose.last().pose()));
                        submit.addProperty("context", args[1].toString());
                        submit.addProperty("light", (int) args[2]);
                    }
                    case "submitMovingBlock", "submitBlockModel" -> {
                        var pose = (PoseStack) args[0];
                        submit.add("matrix", matrix(pose.last().pose()));
                    }
                    case "submitCrumblingOverlay" -> { return null; }
                    default -> { }
                }
                submits.add(submit);
                return null;
            }
        };
        return (SubmitNodeCollector) Proxy.newProxyInstance(SubmitNodeCollector.class.getClassLoader(), new Class<?>[] {SubmitNodeCollector.class}, handler);
    }

    /**
     * The level an End gateway reads: the gateway and the case's neighbour
     * blocks (air elsewhere), its light, build height and game time. Allocated without running
     * Level's constructor; everything else is unsupported.
     */
    static final class CaseLevel extends net.minecraft.world.level.Level {
        Map<BlockPos, BlockState> blocks;
        int minY, height, sky, block;
        long gameTime;
        CaseLevel() { super(null, null, null, null, false, false, 0L, 0); }
        @Override public BlockState getBlockState(BlockPos p) { return blocks.getOrDefault(p, net.minecraft.world.level.block.Blocks.AIR.defaultBlockState()); }
        @Override public int getMinY() { return minY; }
        @Override public int getHeight() { return height; }
        @Override public long getGameTime() { return gameTime; }
        @Override public int getBrightness(net.minecraft.world.level.LightLayer layer, BlockPos p) { return layer == net.minecraft.world.level.LightLayer.SKY ? sky : block; }
        @Override public net.minecraft.world.clock.ClockManager clockManager() { throw new UnsupportedOperationException(); }
        @Override public void destroyBlockProgress(int p0, BlockPos p1, int p2) { throw new UnsupportedOperationException(); }
        @Override public java.util.Collection dragonParts() { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.world.flag.FeatureFlagSet enabledFeatures() { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.world.attribute.EnvironmentAttributeSystem environmentAttributes() { throw new UnsupportedOperationException(); }
        @Override public void explode(net.minecraft.world.entity.Entity p0, net.minecraft.world.damagesource.DamageSource p1, net.minecraft.world.level.ExplosionDamageCalculator p2, double p3, double p4, double p5, float p6, boolean p7, net.minecraft.world.level.Level.ExplosionInteraction p8, net.minecraft.core.particles.ParticleOptions p9, net.minecraft.core.particles.ParticleOptions p10, net.minecraft.util.random.WeightedList p11, net.minecraft.core.Holder p12) { throw new UnsupportedOperationException(); }
        @Override public void gameEvent(net.minecraft.core.Holder p0, net.minecraft.world.phys.Vec3 p1, net.minecraft.world.level.gameevent.GameEvent.Context p2) { throw new UnsupportedOperationException(); }
        @Override public String gatherChunkSourceStats() { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.world.ticks.LevelTickAccess getBlockTicks() { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.world.level.chunk.ChunkSource getChunkSource() { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.world.level.entity.LevelEntityGetter getEntities() { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.world.entity.Entity getEntity(int p0) { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.world.ticks.LevelTickAccess getFluidTicks() { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.world.level.saveddata.maps.MapItemSavedData getMapData(net.minecraft.world.level.saveddata.maps.MapId p0) { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.world.level.storage.LevelData.RespawnData getRespawnData() { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.world.scores.Scoreboard getScoreboard() { throw new UnsupportedOperationException(); }
        @Override public int getSeaLevel() { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.core.Holder getUncachedNoiseBiome(int p0, int p1, int p2) { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.world.level.border.WorldBorder getWorldBorder() { throw new UnsupportedOperationException(); }
        @Override public void levelEvent(net.minecraft.world.entity.Entity p0, int p1, BlockPos p2, int p3) { throw new UnsupportedOperationException(); }
        @Override public void playSeededSound(net.minecraft.world.entity.Entity p0, net.minecraft.world.entity.Entity p1, net.minecraft.core.Holder p2, net.minecraft.sounds.SoundSource p3, float p4, float p5, long p6) { throw new UnsupportedOperationException(); }
        @Override public void playSeededSound(net.minecraft.world.entity.Entity p0, double p1, double p2, double p3, net.minecraft.core.Holder p4, net.minecraft.sounds.SoundSource p5, float p6, float p7, long p8) { throw new UnsupportedOperationException(); }
        @Override public java.util.List players() { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.world.item.crafting.RecipeAccess recipeAccess() { throw new UnsupportedOperationException(); }
        @Override public void sendBlockUpdated(BlockPos p0, BlockState p1, BlockState p2, int p3) { throw new UnsupportedOperationException(); }
        @Override public void setRespawnData(net.minecraft.world.level.storage.LevelData.RespawnData p0) { throw new UnsupportedOperationException(); }
        @Override public net.minecraft.world.TickRateManager tickRateManager() { throw new UnsupportedOperationException(); }
    }

    static BlockState parseState(String spec) throws Exception {
        return BlockStateParser.parseForBlock(BuiltInRegistries.BLOCK, spec, false).blockState();
    }

    static Field field(Class<?> type, String name) throws NoSuchFieldException {
        for (Class<?> c = type; c != null; c = c.getSuperclass()) {
            try {
                Field f = c.getDeclaredField(name);
                f.setAccessible(true);
                return f;
            } catch (NoSuchFieldException e) {
            }
        }
        throw new NoSuchFieldException(name + " on " + type);
    }

    static JsonArray glyphs(FormattedCharSequence sequence, float x, float y, int color, int light, Matrix4f matrix) {
        var result = new JsonArray();
        float[] pen = {x};
        sequence.accept((index, style, c) -> {
            var glyph = new JsonObject();
            glyph.addProperty("text", new String(Character.toChars(c)));
            glyph.addProperty("x", pen[0]); glyph.addProperty("y", y);
            glyph.addProperty("color", 0xff000000 | (style.getColor() == null ? color : style.getColor().getValue()));
            glyph.addProperty("light", light); glyph.add("matrix", matrix(matrix));
            var flags = new JsonArray();
            for (boolean b : new boolean[] {style.isBold(), style.isItalic(), style.isUnderlined(), style.isStrikethrough(), style.isObfuscated()}) flags.add(b);
            glyph.add("style", flags); result.add(glyph);
            pen[0] += font.getSplitter().stringWidth(FormattedCharSequence.codepoint(c, style));
            return true;
        });
        return result;
    }

    static JsonArray textVertices(FormattedCharSequence sequence, float x, float y, int color, int outline, int light) {
        var verts = new ArrayList<float[]>();
        var consumer = recorder(verts);
        var pose = new Matrix4f();
        var visitor = new Font.GlyphVisitor() {
            @Override public void acceptRenderable(net.minecraft.client.gui.font.TextRenderable renderable) {
                renderable.render(pose, consumer, light, false);
            }
        };
        if (outline != 0) {
            font.prepare8xTextOutline(sequence, x, y, outline).visit(visitor);
            pose.translate(0, 0, 0.05F); // Horizon's POLYGON_OFFSET substitute.
        }
        font.prepareText(sequence, x, y, color, false, false, 0).visit(visitor);
        var result = new JsonArray();
        for (float[] v : verts) {
            var point = new JsonArray();
            point.add(v[0]); point.add(v[1]); point.add(v[2]);
            point.add(Integer.toUnsignedLong(Float.floatToRawIntBits(v[3])));
            result.add(point);
        }
        return result;
    }

    static DyeColor dye(String name) {
        return DyeColor.byName(name, DyeColor.BLACK);
    }

    static SignText signText(JsonObject side) {
        var lines = side.getAsJsonArray("lines");
        var messages = new ArrayList<Component>();
        for (int i = 0; i < 4; i++) messages.add(i < lines.size()
            ? ComponentSerialization.CODEC.parse(JsonOps.INSTANCE, lines.get(i)).getOrThrow() : Component.empty());
        return new SignText(messages, messages, dye(side.get("color").getAsString()), side.get("glowing").getAsBoolean());
    }

    /** Block-entity fields that vanilla ticks; set from the case before extraction. */
    static void apply(BlockEntity be, JsonObject c) throws Exception {
        if (be instanceof BellBlockEntity bell) {
            bell.ticks = c.has("bellTicks") ? c.get("bellTicks").getAsInt() : 0;
            bell.shaking = c.has("shake");
            if (bell.shaking) bell.clickDirection = Direction.byName(c.get("shake").getAsString());
        } else if (be instanceof ShulkerBoxBlockEntity) {
            field(ShulkerBoxBlockEntity.class, "progressOld").setFloat(be, c.has("progressOld") ? c.get("progressOld").getAsFloat() : 0.0F);
            field(ShulkerBoxBlockEntity.class, "progress").setFloat(be, c.has("progress") ? c.get("progress").getAsFloat() : 0.0F);
        } else if (be instanceof ConduitBlockEntity conduit && c.has("conduit")) {
            var k = c.getAsJsonObject("conduit");
            conduit.tickCount = k.get("tickCount").getAsInt();
            field(ConduitBlockEntity.class, "isActive").setBoolean(be, k.get("active").getAsBoolean());
            field(ConduitBlockEntity.class, "isHunting").setBoolean(be, k.get("hunting").getAsBoolean());
            field(ConduitBlockEntity.class, "activeRotation").setFloat(be, k.get("activeRotation").getAsFloat());
        } else if (be instanceof EnchantingTableBlockEntity table && c.has("book")) {
            var b = c.getAsJsonObject("book");
            table.time = b.get("time").getAsInt();
            table.flip = b.get("flip").getAsFloat(); table.oFlip = b.get("oFlip").getAsFloat();
            table.open = b.get("open").getAsFloat(); table.oOpen = b.get("oOpen").getAsFloat();
            table.rot = b.get("rot").getAsFloat(); table.oRot = b.get("oRot").getAsFloat();
        } else if (be instanceof SkullBlockEntity && c.has("skullAnimation")) {
            var a = c.getAsJsonArray("skullAnimation");
            field(SkullBlockEntity.class, "isAnimating").setBoolean(be, a.get(0).getAsBoolean());
            field(SkullBlockEntity.class, "animationTickCount").setInt(be, a.get(1).getAsInt());
        } else if (be instanceof DecoratedPotBlockEntity && c.has("sherds")) {
            var s = c.getAsJsonArray("sherds");
            var items = new ArrayList<Optional<ItemStackTemplate>>();
            for (var e : s) {
                var id = e.getAsString();
                items.add(id.isEmpty() ? Optional.empty()
                    : Optional.of(new ItemStackTemplate(BuiltInRegistries.ITEM.getValue(Identifier.parse(id)))));
            }
            field(DecoratedPotBlockEntity.class, "decorations").set(be, new PotDecorations(items.get(0), items.get(1), items.get(2), items.get(3)));
        } else if (be instanceof TheEndGatewayBlockEntity) {
            // Age and teleportCooldown (synced and ticked by the client), and the level the renderer reads.
            var g = c.has("gateway") ? c.getAsJsonObject("gateway") : new JsonObject();
            field(TheEndGatewayBlockEntity.class, "age").setLong(be, g.has("age") ? g.get("age").getAsLong() : 0L);
            field(TheEndGatewayBlockEntity.class, "teleportCooldown").setInt(be, g.has("cooldown") ? g.get("cooldown").getAsInt() : 0);
            var level = (CaseLevel) allocate.invoke(unsafe, CaseLevel.class);
            var height = c.has("level") ? c.getAsJsonArray("level") : null;
            level.minY = height == null ? 0 : height.get(0).getAsInt();
            level.height = height == null ? 256 : height.get(1).getAsInt();
            level.gameTime = c.has("gameTime") ? c.get("gameTime").getAsLong() : 0L;
            var light = c.getAsJsonArray("light");
            level.sky = light.get(0).getAsInt();
            level.block = light.get(1).getAsInt();
            level.blocks = new HashMap<>();
            level.blocks.put(be.getBlockPos(), be.getBlockState());
            if (c.has("neighbours")) {
                for (var e : c.getAsJsonObject("neighbours").entrySet())
                    level.blocks.put(be.getBlockPos().relative(Direction.byName(e.getKey())), parseState(e.getValue().getAsString()));
            }
            be.setLevel(level);
        } else if (be instanceof BannerBlockEntity && c.has("patterns")) {
            field(BannerBlockEntity.class, "patterns").set(be, patterns(c.getAsJsonArray("patterns")));
        }
    }

    /** [[pattern id, dye], ...] as BannerPatternLayers. */
    static BannerPatternLayers patterns(JsonArray list) {
        var builder = new BannerPatternLayers.Builder();
        var registry = ParityData.access.lookupOrThrow(Registries.BANNER_PATTERN);
        for (var e : list) {
            var p = e.getAsJsonArray();
            var holder = registry.getOrThrow(ResourceKey.create(Registries.BANNER_PATTERN, Identifier.parse(p.get(0).getAsString())));
            builder.add(holder, dye(p.get(1).getAsString()));
        }
        return builder.build();
    }

    /**
     * LoomScreen's result preview (GuiGraphicsExtractor.bannerPattern): the
     * picture-in-picture pose of PictureInPictureRenderer.prepare for its 20x40
     * rectangle at GUI scale 1 (translate(w / 2, h), scale(16, 16, -16)), then
     * GuiBannerResultRenderer.renderToTexture without its lighting setup
     * (Lighting.Entry.ITEMS_FLAT, applied by the harness).
     */
    static void loomResult(JsonObject loom, BannerFlagModel flag, SpriteGetter sprites, SubmitNodeCollector collector) {
        var pose = new PoseStack();
        pose.translate(20 / 2.0F, 40.0F, 0.0F);
        pose.scale(16.0F, 16.0F, -16.0F);
        pose.translate(0.0F, 0.25F, 0.0F);
        collector.submitModel(flag, 0.0F, pose, 15728880, OverlayTexture.NO_OVERLAY, -1, Sheets.BANNER_BASE, sprites, 0);
        BannerRenderer.submitPatterns(sprites, pose, collector, 15728880, OverlayTexture.NO_OVERLAY, flag, 0.0F, true,
            dye(loom.get("base").getAsString()), patterns(loom.getAsJsonArray("patterns")));
    }

    /** Render-state values vanilla reads from the level (absent here). */
    static void patch(BlockEntityRenderState rs, BlockState state, JsonObject c) {
        if (rs instanceof ChestRenderState chest) {
            chest.type = state.hasProperty(net.minecraft.world.level.block.ChestBlock.TYPE)
                ? state.getValue(net.minecraft.world.level.block.ChestBlock.TYPE) : ChestType.SINGLE;
            chest.facing = state.getValue(net.minecraft.world.level.block.ChestBlock.FACING);
            chest.open = c.has("open") ? c.get("open").getAsFloat() : 0.0F;
        } else if (rs instanceof DecoratedPotRenderState pot && c.has("wobble")) {
            var w = c.getAsJsonArray("wobble");
            pot.wobbleStyle = DecoratedPotBlockEntity.WobbleStyle.valueOf(w.get(0).getAsString());
            pot.wobbleProgress = w.get(1).getAsFloat();
        }
    }

    @SuppressWarnings({"unchecked", "rawtypes"})
    public static void main(String[] args) throws Exception {
        ParityData.init();
        var unsafeField = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe");
        unsafeField.setAccessible(true);
        unsafe = unsafeField.get(null);
        allocate = unsafe.getClass().getMethod("allocateInstance", Class.class);
        var resources = ParityFont.resources();
        font = VanillaTextDisplayRenderOracle.font(resources);
        net.minecraft.locale.Language.inject(net.minecraft.client.resources.language.ClientLanguage.loadFrom(resources, List.of("en_us"), false));
        var models = new EntityModelSet(LayerDefinitions.createRoots());
        SpriteGetter sprites = id -> {
            try {
                var sprite = (TextureAtlasSprite) allocate.invoke(unsafe, TextureAtlasSprite.class);
                SPRITES.put(sprite, id);
                return sprite;
            } catch (Exception e) {
                throw new RuntimeException(e);
            }
        };
        // Campfires and brushable blocks resolve their (here always empty) items: an empty
        // stack only clears the render state and never reaches the model manager.
        var items = new net.minecraft.client.renderer.item.ItemModelResolver(null);
        var context = new BlockEntityRendererProvider.Context(null, null, items, null, models, font, sprites, null);
        Map<Object, BlockEntityRenderer> renderers = (Map) BlockEntityRenderers.createEntityRenderers(context);
        var loomFlag = new BannerFlagModel(models.bakeLayer(net.minecraft.client.model.geom.ModelLayers.STANDING_BANNER_FLAG));
        var gson = new Gson();
        var out = new StringBuilder();
        for (String line : Files.readAllLines(Path.of(args[0]))) {
            if (line.isBlank()) continue;
            var c = JsonParser.parseString(line).getAsJsonObject();
            var result = new JsonObject();
            result.add("case", c);
            try {
                if (c.has("loom")) {
                    var submits = new JsonArray();
                    loomResult(c.getAsJsonObject("loom"), loomFlag, sprites, collector(submits));
                    result.add("submits", submits);
                    out.append(gson.toJson(result)).append('\n');
                    continue;
                }
                BlockState state = parseState(c.get("block").getAsString());
                var p = c.getAsJsonArray("pos");
                var pos = new BlockPos(p.get(0).getAsInt(), p.get(1).getAsInt(), p.get(2).getAsInt());
                var be = ((EntityBlock) state.getBlock()).newBlockEntity(pos, state);
                if (be == null) throw new IllegalStateException("no block entity");
                apply(be, c);
                var renderer = renderers.get(be.getType());
                if (renderer == null) throw new IllegalStateException("no renderer");
                float partial = c.get("partial").getAsFloat();
                var cam = c.getAsJsonArray("camera");
                var cameraPos = new Vec3(cam.get(0).getAsDouble(), cam.get(1).getAsDouble(), cam.get(2).getAsDouble());
                var rs = (BlockEntityRenderState) renderer.createRenderState();
                if (rs instanceof BeaconRenderState beacon) {
                    // BeaconRenderer.extract reads Minecraft.getInstance().player (scoping); its level-free part:
                    BlockEntityRenderState.extractBase(be, rs, null);
                    beacon.animationTime = Math.floorMod(c.get("gameTime").getAsLong(), 40) + partial;
                    var sections = new ArrayList<BeaconRenderState.Section>();
                    for (var e : c.getAsJsonArray("beam")) sections.add(new BeaconRenderState.Section(e.getAsJsonArray().get(0).getAsInt(), e.getAsJsonArray().get(1).getAsInt()));
                    beacon.sections = sections;
                    float distance = (float) cameraPos.subtract(Vec3.atCenterOf(pos)).horizontalDistance();
                    beacon.beamRadiusScale = Math.max(1.0F, distance / 96.0F);
                } else
                if (rs instanceof SignRenderState sign) {
                    // AbstractSignRenderer.extractRenderState reads Minecraft.getInstance(); its level-free part:
                    var signBe = (SignBlockEntity) be;
                    BlockEntityRenderState.extractBase(be, rs, null);
                    sign.maxTextLineWidth = signBe.getMaxTextLineWidth();
                    sign.textLineHeight = signBe.getTextLineHeight();
                    var s = c.getAsJsonObject("sign");
                    sign.frontText = signText(s.getAsJsonObject("front"));
                    sign.backText = signText(s.getAsJsonObject("back"));
                    sign.drawOutline = s.get("near").getAsBoolean();
                    if (rs instanceof StandingSignRenderState standing) {
                        standing.attachmentType = net.minecraft.world.level.block.PlainSignBlock.getAttachmentPoint(state);
                        sign.transformations = state.getBlock() instanceof net.minecraft.world.level.block.WallSignBlock
                            ? StandingSignRenderer.TRANSFORMATIONS.wallTransformation(state.getValue(net.minecraft.world.level.block.WallSignBlock.FACING))
                            : StandingSignRenderer.TRANSFORMATIONS.freeTransformations(state.getValue(net.minecraft.world.level.block.StandingSignBlock.ROTATION));
                    } else {
                        ((HangingSignRenderState) rs).attachmentType = net.minecraft.world.level.block.HangingSignBlock.getAttachmentPoint(state);
                        sign.transformations = state.getBlock() instanceof net.minecraft.world.level.block.WallHangingSignBlock
                            ? HangingSignRenderer.TRANSFORMATIONS.wallTransformation(state.getValue(net.minecraft.world.level.block.WallHangingSignBlock.FACING))
                            : HangingSignRenderer.TRANSFORMATIONS.freeTransformations(state.getValue(net.minecraft.world.level.block.CeilingHangingSignBlock.ROTATION));
                    }
                } else {
                    renderer.extractRenderState(be, rs, partial, cameraPos, null);
                }
                var light = c.getAsJsonArray("light");
                rs.lightCoords = light.get(1).getAsInt() << 4 | light.get(0).getAsInt() << 20;
                patch(rs, state, c);
                var camera = new CameraRenderState();
                camera.pos = cameraPos;
                float yaw = c.has("yaw") ? c.get("yaw").getAsFloat() : 0.0F, pitch = c.has("pitch") ? c.get("pitch").getAsFloat() : 0.0F;
                // Camera.setRotation.
                camera.orientation = new Quaternionf().rotationYXZ((float) Math.PI - yaw * (float) (Math.PI / 180.0), -pitch * (float) (Math.PI / 180.0), 0.0F);
                var submits = new JsonArray();
                renderer.submit(rs, new PoseStack(), collector(submits), camera);
                result.add("submits", submits);
            } catch (Throwable error) {
                var cause = error instanceof InvocationTargetException && error.getCause() != null ? error.getCause() : error;
                var sw = new java.io.StringWriter();
                cause.printStackTrace(new java.io.PrintWriter(sw));
                result.addProperty("error", sw.toString().lines().limit(6).reduce("", (a, b) -> a + b + " | "));
            }
            out.append(gson.toJson(result)).append('\n');
        }
        Files.writeString(Path.of(args[1]), out.toString(), StandardCharsets.UTF_8);
    }
}
