// SPDX-License-Identifier: MIT OR Apache-2.0
// JVM probes for M0 (run with the single-file source launcher: java Test.java <mode>).
import java.time.Instant;
import java.util.LinkedHashMap;
import java.util.Map;
import java.util.concurrent.*;
import java.util.concurrent.locks.LockSupport;
import java.util.concurrent.locks.ReentrantLock;

public class Test {
    interface Body { void run() throws Exception; }

    public static void main(String[] args) throws Exception {
        if (args[0].equals("now")) {
            System.out.printf("{\"currentTimeMillis\":%.3f,\"instant_now\":%.3f}%n",
                System.currentTimeMillis() / 1000.0, Instant.now().toEpochMilli() / 1000.0);
            return;
        }
        Map<String, Body> tests = new LinkedHashMap<>();
        tests.put("Thread.sleep", () -> Thread.sleep(1000));
        tests.put("Object.wait", () -> { Object o = new Object(); synchronized (o) { o.wait(1000); } });
        tests.put("LockSupport.parkNanos", () -> LockSupport.parkNanos(1_000_000_000L));
        tests.put("LockSupport.parkUntil_wallclock", () -> LockSupport.parkUntil(System.currentTimeMillis() + 1000));
        tests.put("ReentrantLock.tryLock", () -> {
            ReentrantLock l = new ReentrantLock();
            Thread h = new Thread(() -> { l.lock(); try { Thread.sleep(10000); } catch (InterruptedException e) {} });
            h.setDaemon(true); h.start(); Thread.sleep(100);
            l.tryLock(1, TimeUnit.SECONDS);
        });
        tests.put("CountDownLatch.await", () -> new CountDownLatch(1).await(1, TimeUnit.SECONDS));
        tests.put("ScheduledExecutor.schedule", () -> {
            ScheduledExecutorService s = Executors.newSingleThreadScheduledExecutor();
            s.schedule(() -> {}, 1, TimeUnit.SECONDS).get();
            s.shutdownNow();
        });
        tests.put("CompletableFuture.get_timeout", () -> {
            try { new CompletableFuture<Void>().get(1, TimeUnit.SECONDS); } catch (TimeoutException e) {}
        });
        for (Map.Entry<String, Body> e : tests.entrySet()) {
            long t0 = System.nanoTime();
            Thread t = new Thread(() -> { try { e.getValue().run(); } catch (Exception ex) {} });
            t.setDaemon(true); t.start(); t.join(6000);
            long ms = (System.nanoTime() - t0) / 1_000_000;
            String st = t.isAlive() ? "HANG" : ms < 900 ? "EARLY" : ms > 2500 ? "SLOW" : "OK";
            System.out.printf("{\"test\":\"%s\",\"elapsed_ms\":%d,\"status\":\"%s\"}%n", e.getKey(), ms, st);
        }
        System.exit(0);
    }
}
