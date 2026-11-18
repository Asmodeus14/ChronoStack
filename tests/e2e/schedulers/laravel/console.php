<?php
// SPDX-License-Identifier: MIT OR Apache-2.0
// Laravel scheduler (`php artisan schedule:work`): daily at 03:00 UTC; counts
// runs in Redis (laravel:runs). Default settings, as most applications use it.

use Illuminate\Support\Facades\Schedule;

Schedule::call(function () {
    (new Predis\Client(['host' => 'redis']))->incr('laravel:runs');
})->dailyAt('03:00')->timezone('UTC');
