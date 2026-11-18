// SPDX-License-Identifier: MIT OR Apache-2.0
// Spring @Scheduled: daily at 03:00 UTC; counts runs in Redis (spring:runs).
// Default scheduler settings, as most applications use it.
import org.springframework.context.annotation.AnnotationConfigApplicationContext;
import org.springframework.context.annotation.Configuration;
import org.springframework.scheduling.annotation.EnableScheduling;
import org.springframework.scheduling.annotation.Scheduled;
import redis.clients.jedis.JedisPooled;

@Configuration
@EnableScheduling
public class Daily {
    private static final JedisPooled REDIS = new JedisPooled("redis", 6379);

    @Scheduled(cron = "0 0 3 * * *", zone = "UTC")
    public void daily() {
        REDIS.incr("spring:runs");
    }

    public static void main(String[] args) throws InterruptedException {
        new AnnotationConfigApplicationContext(Daily.class);
        Thread.currentThread().join();
    }
}
