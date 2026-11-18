# SPDX-License-Identifier: MIT OR Apache-2.0
# sidekiq-cron: daily at 03:00 UTC; counts runs in Redis (sidekiqcron:runs).
# Default settings (30 s poll interval), as most applications use it.
require "sidekiq"
require "sidekiq-cron"

class DailyJob
  include Sidekiq::Job

  def perform
    Sidekiq.redis { |r| r.call("INCR", "sidekiqcron:runs") }
  end
end

Sidekiq.configure_server do |config|
  config.on(:startup) do
    Sidekiq::Cron::Job.create(name: "daily", cron: "0 3 * * * UTC", class: "DailyJob")
  end
end
