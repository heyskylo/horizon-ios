package dev.horizonlab.parity;

import java.lang.classfile.ClassFile;
import java.lang.classfile.MethodModel;
import java.lang.constant.ClassDesc;
import java.lang.constant.ConstantDescs;
import java.lang.constant.MethodTypeDesc;
import java.lang.instrument.ClassFileTransformer;
import java.lang.instrument.Instrumentation;
import java.security.ProtectionDomain;

/**
 * Test-only Java agent for vanilla oracles (docs/parity-workflow.md, "Randoms that repeat"); tools/seed_agent.py
 * builds it. It rewrites RandomSupport.generateUniqueSeed, the seed of every RandomSource.create(), to step the
 * calling thread's own uniquifier, as vanilla steps its shared one, and to leave out the System.nanoTime term. A
 * harness sets the server thread's uniquifier at its sync step; Horizon's HORIZON_TEST_SEEDS does the same.
 */
public final class SeedAgent {
    private static final String TARGET = "net/minecraft/world/level/levelgen/RandomSupport";
    // RandomSupport.SEED_UNIQUIFIER's start and step.
    private static final ThreadLocal<long[]> UNIQUIFIER = ThreadLocal.withInitial(() -> new long[] {8682522807148012L});
    private static final long STEP = 1181783497276652981L;
    private static volatile boolean installed;

    public static void premain(String options, Instrumentation instrumentation) {
        instrumentation.addTransformer(new ClassFileTransformer() {
            @Override
            public byte[] transform(ClassLoader loader, String name, Class<?> redefined, ProtectionDomain domain, byte[] bytes) {
                if (!TARGET.equals(name)) {
                    return null;
                }
                try {
                    return rewrite(bytes);
                } catch (Throwable error) {
                    // The JVM ignores a transformer's exceptions: stop rather than run with the clock.
                    error.printStackTrace();
                    Runtime.getRuntime().halt(3);
                    return null;
                }
            }
        });
    }

    private static byte[] rewrite(byte[] bytes) {
        ClassFile files = ClassFile.of();
        boolean[] found = {false};
        byte[] rewritten = files.transformClass(files.parse(bytes), (builder, element) -> {
            if (element instanceof MethodModel method && method.methodName().equalsString("generateUniqueSeed")
                    && method.methodType().equalsString("()J")) {
                found[0] = true;
                builder.withMethodBody(method.methodName(), method.methodType(), method.flags().flagsMask(), code -> code
                    .invokestatic(ClassDesc.of(SeedAgent.class.getName()), "nextSeed", MethodTypeDesc.of(ConstantDescs.CD_long))
                    .lreturn());
            } else {
                builder.with(element);
            }
        });
        if (!found[0]) {
            throw new IllegalStateException("RandomSupport has no generateUniqueSeed()J");
        }
        installed = true;
        return rewritten;
    }

    /** The rewritten RandomSupport.generateUniqueSeed. */
    public static long nextSeed() {
        long[] uniquifier = UNIQUIFIER.get();
        return uniquifier[0] *= STEP;
    }

    /** The calling thread's uniquifier from now on: a harness's sync step. */
    public static void setUniquifier(long value) {
        if (!installed) {
            throw new IllegalStateException("RandomSupport.generateUniqueSeed was not rewritten (no -javaagent?)");
        }
        UNIQUIFIER.get()[0] = value;
    }
}
