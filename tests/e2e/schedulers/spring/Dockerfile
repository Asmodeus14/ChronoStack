# SPDX-License-Identifier: MIT OR Apache-2.0
FROM maven:3.9-eclipse-temurin-21 AS build
WORKDIR /src
COPY pom.xml ./
COPY src ./src
# Maven Central downloads occasionally fail on CI runners: retry each request,
# then the whole build, and keep the output (no -q) so a real error is visible.
RUN for i in 1 2 3; do \
      mvn -B --no-transfer-progress -Daether.connector.http.retryHandler.count=5 \
        package dependency:copy-dependencies -DoutputDirectory=target/lib && exit 0; \
      echo "mvn failed (attempt $i of 3), retrying in 10s"; sleep 10; \
    done; exit 1

FROM eclipse-temurin:21-jre
WORKDIR /app
COPY --from=build /src/target/spring-scheduled-1.jar /app/app.jar
COPY --from=build /src/target/lib /app/lib
CMD ["java", "-cp", "/app/app.jar:/app/lib/*", "Daily"]
