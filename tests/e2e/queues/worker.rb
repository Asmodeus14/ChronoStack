# SPDX-License-Identifier: MIT OR Apache-2.0
require "sidekiq"
class SlowJob
  include Sidekiq::Job
  def perform(id)
    sleep 4
    Sidekiq.redis { |r| r.call("INCR", "done:sidekiq") }
  end
end
