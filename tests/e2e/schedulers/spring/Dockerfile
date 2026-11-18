# SPDX-License-Identifier: MIT OR Apache-2.0
FROM maven:3.9-eclipse-temurin-21 AS build
WORKDIR /src
COPY pom.xml ./
COPY src ./src
RUN mvn -q -B package dependency:copy-dependencies -DoutputDirectory=target/lib

FROM eclipse-temurin:21-jre
WORKDIR /app
COPY --from=build /src/target/spring-scheduled-1.jar /app/app.jar
COPY --from=build /src/target/lib /app/lib
CMD ["java", "-cp", "/app/app.jar:/app/lib/*", "Daily"]
