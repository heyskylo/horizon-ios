package dev.horizonlab.parity;

import com.google.gson.JsonParser;
import java.lang.reflect.Field;
import java.nio.file.Files;
import java.nio.file.Path;
import net.minecraft.client.Minecraft;
import net.minecraft.client.multiplayer.ClientLevel;
import net.minecraft.client.multiplayer.MultiPlayerGameMode;
import net.minecraft.client.player.LocalPlayer;
import net.minecraft.core.BlockPos;
import net.minecraft.core.particles.ParticleOptions;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.sounds.SoundEvent;
import net.minecraft.sounds.SoundSource;
import net.minecraft.util.RandomSource;
import net.minecraft.world.InteractionHand;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.Items;
import net.minecraft.world.level.GameType;
import net.minecraft.world.level.Level;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.CampfireBlock;
import net.minecraft.world.level.block.entity.CampfireBlockEntity;
import net.minecraft.world.level.block.state.BlockState;
import net.minecraft.world.level.block.state.properties.Property;
import net.minecraft.world.level.material.FluidState;
import net.minecraft.world.level.material.Fluids;

/** Invoke shipped marker selection and ambient sampling in a dry barrier world,
 * the campfire block's tick over a world that is all campfire, and the
 * campfire block entity's particles over a stand-in level that records them. */
public final class VanillaMarkerParticlesOracle {
    static Field field(Class<?> type, String name) throws Exception {
        var field = type.getDeclaredField(name);
        field.setAccessible(true);
        return field;
    }
    static String bits(double v) { return String.format("%016x", Double.doubleToLongBits(v)); }
    public static final class Player extends LocalPlayer {
        ItemStack main, off;
        Player() { super(null, null, null, null, null, null, false, null, null); }
        @Override public ItemStack getMainHandItem() { return main; }
        @Override public ItemStack getItemInHand(InteractionHand hand) {
            return hand == InteractionHand.MAIN_HAND ? main : off;
        }
    }
    public static final class Mode extends MultiPlayerGameMode {
        GameType mode;
        Mode() { super(null, null); }
        @Override public GameType getPlayerMode() { return mode; }
    }
    public static final class MarkerLevel extends ClientLevel {
        java.io.BufferedWriter out;
        MarkerLevel() { super(null, null, null, null, 0, 0, null, false, 0L, 0); }
        @Override public BlockState getBlockState(BlockPos pos) { return Blocks.BARRIER.defaultBlockState(); }
        @Override public FluidState getFluidState(BlockPos pos) { return Fluids.EMPTY.defaultFluidState(); }
        @Override public void addParticle(ParticleOptions options, double x, double y, double z,
                                          double dx, double dy, double dz) {
            if (dx != 0 || dy != 0 || dz != 0) throw new AssertionError("marker velocity");
            try {
                out.write("marker " + x + " " + y + " " + z);
                out.newLine();
            } catch (java.io.IOException e) { throw new RuntimeException(e); }
        }
    }
    /** The level a campfire is ticked over: it holds no blocks and records every
     * particle it is asked for by its registry name and position. */
    public static final class CampfireLevel extends ClientLevel {
        java.io.BufferedWriter out;
        CampfireLevel() { super(null, null, null, null, 0, 0, null, false, 0L, 0); }
        @Override public BlockState getBlockState(BlockPos pos) { return Blocks.AIR.defaultBlockState(); }
        @Override public FluidState getFluidState(BlockPos pos) { return Fluids.EMPTY.defaultFluidState(); }
        private void record(ParticleOptions options, double x, double y, double z,
                            double dx, double dy, double dz) {
            try {
                out.write(BuiltInRegistries.PARTICLE_TYPE.getKey(options.getType()).getPath()
                          + " " + bits(x) + " " + bits(y) + " " + bits(z)
                          + " " + bits(dx) + " " + bits(dy) + " " + bits(dz));
                out.newLine();
            } catch (java.io.IOException e) { throw new RuntimeException(e); }
        }
        @Override public void addParticle(ParticleOptions options, double x, double y, double z,
                                          double dx, double dy, double dz) {
            record(options, x, y, z, dx, dy, dz);
        }
        @Override public void addAlwaysVisibleParticle(ParticleOptions options, boolean force,
                                                       double x, double y, double z,
                                                       double dx, double dy, double dz) {
            record(options, x, y, z, dx, dy, dz);
        }
    }
    /** The level a campfire block tick runs over: every position holds the
     * case's campfire, so the whole animateTick loop reaches one, and every
     * particle and local sound it asks for is recorded. */
    public static final class TickLevel extends ClientLevel {
        BlockState state = Blocks.AIR.defaultBlockState();
        /** Every block below y = 64, so a case can put something else under its block. */
        BlockState floor = Blocks.AIR.defaultBlockState();
        java.util.List<String> particles;
        java.util.List<String> sounds;
        net.minecraft.world.attribute.EnvironmentAttributeSystem attributes;
        TickLevel() { super(null, null, null, null, 0, 0, null, false, 0L, 0); }
        @Override public BlockState getBlockState(BlockPos pos) { return pos.getY() < 64 ? floor : state; }
        /** The state's own fluid, as the probe's world reports it: a waterlogged
         * block hands the loop a water source, which it then ticks and drips. */
        @Override public FluidState getFluidState(BlockPos pos) { return getBlockState(pos).getFluidState(); }
        /** Dry weather, as the probe's stand-in world reports it. */
        @Override public boolean isRainingAt(BlockPos pos) { return false; }
        /** The overworld's build height, which a stalactite's root search asks for
         * and a level built without a dimension type cannot answer. */
        @Override public int getMinY() { return -64; }
        @Override public int getHeight() { return 384; }
        @Override public net.minecraft.world.attribute.EnvironmentAttributeSystem environmentAttributes() {
            return attributes;
        }
        @Override public void addParticle(ParticleOptions options, double x, double y, double z,
                                          double dx, double dy, double dz) {
            particles.add(BuiltInRegistries.PARTICLE_TYPE.getKey(options.getType()).getPath()
                          + " " + bits(x) + " " + bits(y) + " " + bits(z)
                          + " " + bits(dx) + " " + bits(dy) + " " + bits(dz));
        }
        @Override public void playLocalSound(double x, double y, double z, SoundEvent sound,
                                             SoundSource source, float volume, float pitch,
                                             boolean distanceDelay) {
            sounds.add(BuiltInRegistries.SOUND_EVENT.getKey(sound)
                       + " " + bits(x) + " " + bits(y) + " " + bits(z)
                       + " " + Integer.toHexString(Float.floatToIntBits(volume))
                       + " " + Integer.toHexString(Float.floatToIntBits(pitch)));
        }
    }
    /** The pinned client's block tags, bound the way a datapack load binds them:
     * a bootstrap-only run leaves every tag empty, and the ticks under test ask
     * about `speleothems`, `impermeable` and the two ambient-sound tags. */
    @SuppressWarnings("unchecked")
    static void bindBlockTags() throws Exception {
        var source = Block.class.getProtectionDomain().getCodeSource().getLocation();
        var raw = new java.util.HashMap<Identifier, java.util.List<String>>();
        try (var file = new java.util.zip.ZipFile(java.nio.file.Path.of(source.toURI()).toFile())) {
            var entries = file.entries();
            while (entries.hasMoreElements()) {
                var entry = entries.nextElement();
                var path = entry.getName();
                var inner = path.indexOf("/tags/block/");
                if (!path.startsWith("data/") || inner < 0 || !path.endsWith(".json")) continue;
                var namespace = path.substring("data/".length(), inner);
                var name = path.substring(inner + "/tags/block/".length(), path.length() - ".json".length());
                var json = JsonParser.parseString(new String(file.getInputStream(entry).readAllBytes(),
                                                             java.nio.charset.StandardCharsets.UTF_8)).getAsJsonObject();
                var values = new java.util.ArrayList<String>();
                for (var value : json.getAsJsonArray("values")) {
                    if (value.isJsonPrimitive()) values.add(value.getAsString());
                    else if (value.isJsonObject() && value.getAsJsonObject().has("id"))
                        values.add(value.getAsJsonObject().get("id").getAsString());
                }
                raw.put(Identifier.fromNamespaceAndPath(namespace, name), values);
            }
        }
        var registry = (net.minecraft.core.WritableRegistry<Block>) BuiltInRegistries.BLOCK;
        var bound = new java.util.HashMap<net.minecraft.tags.TagKey<Block>,
                java.util.List<net.minecraft.core.Holder<Block>>>();
        for (var entry : raw.entrySet()) {
            var holders = new java.util.ArrayList<net.minecraft.core.Holder<Block>>();
            collectTaggedBlocks(raw, registry, entry.getValue(), holders, new java.util.HashSet<>());
            bound.put(net.minecraft.tags.TagKey.create(
                          net.minecraft.core.registries.Registries.BLOCK, entry.getKey()), holders);
        }
        // A frozen registry takes its tags through the reload a datapack load
        // uses: prepare the pending set and apply it, which binds the tag
        // contents and refreshes every holder's tag set.
        var pending = registry.prepareTagReload(
                new net.minecraft.tags.TagLoader.LoadResult<Block>(
                        net.minecraft.core.registries.Registries.BLOCK, bound));
        pending.apply();
    }

    static void collectTaggedBlocks(java.util.Map<Identifier, java.util.List<String>> raw,
                                    net.minecraft.core.WritableRegistry<Block> registry,
                                    java.util.List<String> values,
                                    java.util.List<net.minecraft.core.Holder<Block>> out,
                                    java.util.Set<Identifier> seen) {
        for (var value : values) {
            if (value.startsWith("#")) {
                var tag = Identifier.parse(value.substring(1));
                if (seen.add(tag) && raw.containsKey(tag)) {
                    collectTaggedBlocks(raw, registry, raw.get(tag), out, seen);
                }
                continue;
            }
            // A block the pinned jar does not have is left out, as a datapack
            // load drops an optional entry it cannot resolve.
            registry.get(Identifier.parse(value))
                    .ifPresent(block -> out.add((net.minecraft.core.Holder<Block>) block));
        }
    }
    /** A `minecraft:name[k=v,...]` state through the real block state definition. */
    @SuppressWarnings({"unchecked", "rawtypes"})
    static BlockState parseState(String spec) {
        int bracket = spec.indexOf('[');
        var name = bracket < 0 ? spec : spec.substring(0, bracket);
        Block block = BuiltInRegistries.BLOCK.getValue(Identifier.parse(name));
        var state = block.defaultBlockState();
        if (bracket < 0) return state;
        for (var pair : spec.substring(bracket + 1, spec.length() - 1).split(",")) {
            var parts = pair.split("=");
            Property property = block.getStateDefinition().getProperty(parts[0]);
            if (property == null) throw new AssertionError("no property " + parts[0]);
            state = state.setValue(property, (Comparable) property.getValue(parts[1]).orElseThrow());
        }
        return state;
    }
    public static void main(String[] args) throws Exception {
        net.minecraft.SharedConstants.tryDetectVersion();
        net.minecraft.server.Bootstrap.bootStrap();
        for (var item : BuiltInRegistries.ITEM) {
            item.builtInRegistryHolder().bindComponents(net.minecraft.core.component.DataComponentMap.EMPTY);
        }
        bindBlockTags();
        var allocator = Class.forName("sun.misc.Unsafe");
        var unsafe = field(allocator, "theUnsafe").get(null);
        var allocate = allocator.getMethod("allocateInstance", Class.class);
        var mc = (Minecraft) allocate.invoke(unsafe, Minecraft.class);
        var level = (MarkerLevel) allocate.invoke(unsafe, MarkerLevel.class);
        var campfireLevel = (CampfireLevel) allocate.invoke(unsafe, CampfireLevel.class);
        var tickLevel = (TickLevel) allocate.invoke(unsafe, TickLevel.class);
        var player = (Player) allocate.invoke(unsafe, Player.class);
        var mode = (Mode) allocate.invoke(unsafe, Mode.class);
        field(ClientLevel.class, "minecraft").set(level, mc);
        field(ClientLevel.class, "minecraft").set(campfireLevel, mc);
        field(ClientLevel.class, "minecraft").set(tickLevel, mc);
        tickLevel.attributes = net.minecraft.world.attribute.EnvironmentAttributeSystem.builder().build();
        tickLevel.particles = new java.util.ArrayList<>();
        tickLevel.sounds = new java.util.ArrayList<>();
        mc.player = player;
        mc.gameMode = mode;
        var select = ClientLevel.class.getDeclaredMethod("getMarkerParticleTarget");
        select.setAccessible(true);
        try (var out = Files.newBufferedWriter(Path.of(args[1]))) {
            level.out = out;
            campfireLevel.out = out;
            var lines = Files.readAllLines(Path.of(args[0]));
            for (int index = 0; index < lines.size(); index++) {
                var c = JsonParser.parseString(lines.get(index)).getAsJsonObject();
                var kind = c.has("kind") ? c.get("kind").getAsString() : "marker";
                if (kind.equals("tick")) {
                    // The block's own tick through the whole ClientLevel.animateTick
                    // loop: the level random it is seeded with drives both the
                    // sampled positions and the block's draws, as the probe's one
                    // engine random does.
                    var random = RandomSource.create(c.get("seed").getAsLong());
                    field(Level.class, "random").set(tickLevel, random);
                    tickLevel.state = parseState(c.get("state").getAsString());
                    tickLevel.floor = c.has("floor")
                            ? parseState(c.get("floor").getAsString()) : tickLevel.state;
                    tickLevel.particles.clear();
                    tickLevel.sounds.clear();
                    var pos = new BlockPos.MutableBlockPos();
                    out.write("== " + index + " tick");
                    out.newLine();
                    for (int round = 0; round < 667; round++) {
                        tickLevel.doAnimateTick(0, 64, 0, 16, random, null, pos);
                        tickLevel.doAnimateTick(0, 64, 0, 32, random, null, pos);
                    }
                    for (var line : tickLevel.sounds) {
                        out.write("sound " + line);
                        out.newLine();
                    }
                    for (var line : tickLevel.particles) {
                        out.write(line);
                        out.newLine();
                    }
                    out.write("next " + random.nextInt());
                    out.newLine();
                    continue;
                }
                if (!kind.equals("marker")) {
                    // The campfire cases seed the level random and tick the fire.
                    var random = RandomSource.create(c.get("seed").getAsLong());
                    field(Level.class, "random").set(campfireLevel, random);
                    var pos = new BlockPos(0, 64, 0);
                    out.write("== " + index + " " + kind);
                    out.newLine();
                    if (kind.equals("campfire")) {
                        var state = parseState(c.get("state").getAsString());
                        var entity = new CampfireBlockEntity(pos, state);
                        var slots = c.getAsJsonArray("slots");
                        for (int slot = 0; slot < 4; slot++) {
                            if (slots.get(slot).getAsInt() != 0) {
                                entity.getItems().set(slot, new ItemStack(Items.BEEF));
                            }
                        }
                        // ClientLevel only ticks a lit campfire (CampfireBlock.isLitCampfire).
                        if (state.getValue(CampfireBlock.LIT)) {
                            for (int tick = 0; tick < c.get("ticks").getAsInt(); tick++) {
                                CampfireBlockEntity.particleTick(campfireLevel, pos, state, entity);
                            }
                        }
                    } else {
                        for (int puff = 0; puff < c.get("puffs").getAsInt(); puff++) {
                            CampfireBlock.makeParticles(campfireLevel, pos,
                                                        c.get("signal").getAsBoolean(),
                                                        c.get("smoking").getAsBoolean());
                        }
                    }
                    out.write("next " + random.nextInt());
                    out.newLine();
                    continue;
                }
                mode.mode = GameType.byName(c.get("mode").getAsString());
                player.main = new ItemStack(BuiltInRegistries.ITEM.getValue(
                    Identifier.parse("minecraft:" + c.get("main").getAsString())), c.get("count").getAsInt());
                player.off = new ItemStack(BuiltInRegistries.ITEM.getValue(
                    Identifier.parse("minecraft:" + c.get("off").getAsString())));
                var block = (Block) select.invoke(level);
                out.write(block == null ? "none" : BuiltInRegistries.BLOCK.getKey(block).toString());
                out.newLine();
                var random = RandomSource.create(1057);
                field(Level.class, "random").set(level, random);
                level.animateTick(0, 64, 0);
                out.write("next " + random.nextInt());
                out.newLine();
            }
        }
    }
}
