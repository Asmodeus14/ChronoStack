# SPDX-License-Identifier: MIT OR Apache-2.0
require "sidekiq"
Sidekiq.configure_client { |c| c.redis = { url: ENV.fetch("REDIS_URL") } }
require "./worker"
3.times { |i| SlowJob.perform_async(i) }
puts "queued 3 sidekiq jobs"
