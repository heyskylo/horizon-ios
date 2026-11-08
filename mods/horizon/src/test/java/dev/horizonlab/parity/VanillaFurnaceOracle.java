package dev.horizonlab.parity;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import java.io.PrintWriter;
import java.lang.reflect.Field;
import java.nio.charset.StandardCharsets;
import java.nio.file.*;
import java.util.ArrayList;
import java.util.Comparator;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.TreeMap;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.resources.Identifier;
import net.minecraft.world.flag.FeatureFlags;
import net.minecraft.world.item.ItemStack;
import net.minecraft.world.item.crafting.AbstractCookingRecipe;
import net.minecraft.world.item.crafting.RecipeManager;
import net.minecraft.world.level.block.AbstractFurnaceBlock;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.entity.AbstractFurnaceBlockEntity;
import net.minecraft.world.level.block.state.BlockState;

/**
 * Original oracle for the furnace block entity: a real `RecipeManager` and a
 * block-update level holding a furnace, blast furnace or smoker, ticked with
 * the real `AbstractFurnaceBlockEntity.serverTick`. Fixture lines are
 * "furnace KIND TICKS INPUT FUEL OUTPUT TIMER TOTAL LIT LITTOTAL SPEED
 * CHANGE_AT NEW_INPUT"; `-` is an empty stack and CHANGE_AT -1 means no change.
 * Output is the three slots, the five timers, the LIT state, the `RecipesUsed`
 * counts and each used recipe's experience.
 */
public final class VanillaFurnaceOracle {
    private static Map<String, Block> blocks;

    public static void main(String[] args) throws Exception {
        ParityData.init();
        blocks = Map.of(
                "furnace", Blocks.FURNACE,
                "blast_furnace", Blocks.BLAST_FURNACE,
                "smoker", Blocks.SMOKER);
        VanillaBlockUpdateOracle.AIR = Blocks.AIR.defaultBlockState();
        var allocator = Class.forName("sun.misc.Unsafe");
        var unsafeField = allocator.getDeclaredField("theUnsafe");
        unsafeField.setAccessible(true);
        VanillaBlockUpdateOracle.unsafe = unsafeField.get(null);
        VanillaBlockUpdateOracle.allocate = allocator.getMethod("allocateInstance", Class.class);
        var recipes = new RecipeManager(ParityData.lookup);
        recipes.finalizeRecipeLoading(FeatureFlags.VANILLA_SET);
        VanillaBlockUpdateOracle.Server.recipes = recipes;
        Map<String, Float> experience = new TreeMap<>();
        for (var holder : recipes.getRecipes()) {
            if (holder.value() instanceof AbstractCookingRecipe cooking) {
                experience.put(holder.id().identifier().toString(), cooking.experience());
            }
        }
        Path in = Path.of(args[0]), out = Path.of(args[1]);
        try (var w = new PrintWriter(Files.newBufferedWriter(out, StandardCharsets.UTF_8));
             var lines = Files.newBufferedReader(in, StandardCharsets.UTF_8)) {
            int n = 0;
            for (String line = lines.readLine(); line != null; line = lines.readLine()) {
                if (line.startsWith("#")) continue;
                w.println(run(n++, line, recipes, experience));
            }
        }
        System.exit(0);
    }

    private static Field field(Class<?> c, String name) throws Exception {
        Field f = c.getDeclaredField(name);
        f.setAccessible(true);
        return f;
    }

    private static String run(int n, String line, RecipeManager recipes, Map<String, Float> experience)
            throws Exception {
        String[] f = line.split(" ");
        Block block = blocks.get(f[1]);
        int ticks = Integer.parseInt(f[2]);
        ItemStack input = ParityData.stack(f[3]);
        ItemStack fuel = ParityData.stack(f[4]);
        ItemStack output = ParityData.stack(f[5]);
        int timer = Integer.parseInt(f[6]);
        int total = Integer.parseInt(f[7]);
        int lit = Integer.parseInt(f[8]);
        int litTotal = Integer.parseInt(f[9]);
        float speed = Float.parseFloat(f[10]);
        int changeAt = Integer.parseInt(f[11]);
        ItemStack newInput = ParityData.stack(f[12]);

        JsonObject caseJson = new JsonObject();
        JsonArray cells = new JsonArray();
        JsonArray cell = new JsonArray();
        cell.add(0);
        cell.add(0);
        cell.add(0);
        cell.add(Block.getId(block.defaultBlockState()));
        cells.add(cell);
        caseJson.add("cells", cells);
        caseJson.addProperty("seed", 0);
        VanillaBlockUpdateOracle.World w = VanillaBlockUpdateOracle.build(caseJson);
        w.registries = ParityData.access;
        var pos = new BlockPos(0, 0, 0);
        BlockState state = w.getBlockState(pos);
        var be = (AbstractFurnaceBlockEntity) w.getBlockEntity(pos);
        be.setItem(0, input);
        be.setItem(1, fuel);
        be.setItem(2, output);
        field(AbstractFurnaceBlockEntity.class, "cookingTimer").setInt(be, timer);
        field(AbstractFurnaceBlockEntity.class, "cookingTotalTime").setInt(be, total);
        field(AbstractFurnaceBlockEntity.class, "litTimeRemaining").setInt(be, lit);
        field(AbstractFurnaceBlockEntity.class, "litTotalTime").setInt(be, litTotal);
        field(AbstractFurnaceBlockEntity.class, "speedMultiplier").setFloat(be, speed);
        for (int t = 0; t < ticks; t++) {
            if (t == changeAt) be.setItem(0, newInput);
            state = w.getBlockState(pos);
            AbstractFurnaceBlockEntity.serverTick(w, pos, state, be);
        }
        if (changeAt == ticks) be.setItem(0, newInput);
        state = w.getBlockState(pos);
        StringBuilder sb = new StringBuilder(Integer.toString(n));
        sb.append(" kind=").append(f[1]);
        for (int i = 0; i < 3; i++) sb.append(" s").append(i).append('=').append(ParityData.canon(be.getItem(i)));
        sb.append(" timer=").append(field(AbstractFurnaceBlockEntity.class, "cookingTimer").getInt(be))
          .append(" total=").append(field(AbstractFurnaceBlockEntity.class, "cookingTotalTime").getInt(be))
          .append(" lit=").append(field(AbstractFurnaceBlockEntity.class, "litTimeRemaining").getInt(be))
          .append(" littotal=").append(field(AbstractFurnaceBlockEntity.class, "litTotalTime").getInt(be))
          .append(" speed=").append(String.format(Locale.ROOT, "%.3f",
                  field(AbstractFurnaceBlockEntity.class, "speedMultiplier").getFloat(be)))
          .append(" state=").append(state.getValue(AbstractFurnaceBlock.LIT));
        @SuppressWarnings("unchecked")
        Map<Object, Integer> used = (Map<Object, Integer>) field(AbstractFurnaceBlockEntity.class, "recipesUsed").get(be);
        List<String> ids = new ArrayList<>();
        for (Object key : used.keySet()) ids.add(key.toString().replaceAll(".* / ", "").replaceAll("\\]$", ""));
        ids.sort(Comparator.naturalOrder());
        sb.append(" used=");
        for (int i = 0; i < ids.size(); i++) {
            if (i > 0) sb.append(',');
            String id = ids.get(i);
            sb.append(id).append('=').append(used.get(keyFor(used, id)));
        }
        sb.append(" xp=");
        for (int i = 0; i < ids.size(); i++) {
            if (i > 0) sb.append(',');
            String id = ids.get(i);
            sb.append(id).append('=').append(experience.getOrDefault(id, 0.0f));
        }
        return sb.toString();
    }

    private static Object keyFor(Map<Object, Integer> used, String id) {
        for (Object key : used.keySet()) {
            if (key.toString().replaceAll(".* / ", "").replaceAll("\\]$", "").equals(id)) return key;
        }
        return null;
    }
}
