# SPDX-License-Identifier: MIT OR Apache-2.0
FROM php:8.3-cli
RUN apt-get update && apt-get install -y --no-install-recommends git unzip && rm -rf /var/lib/apt/lists/*
COPY --from=composer:2 /usr/bin/composer /usr/bin/composer
RUN composer create-project --no-interaction --prefer-dist laravel/laravel /app "^12.0" \
 && cd /app && composer require --no-interaction "predis/predis:^2.3"
COPY console.php /app/routes/console.php
WORKDIR /app
CMD ["php", "artisan", "schedule:work"]
