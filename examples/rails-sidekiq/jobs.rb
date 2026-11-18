# SPDX-License-Identifier: MIT OR Apache-2.0
# A trial that ends after 14 days. Signing up schedules a job for that day; the
# job "sends" the email by pushing it onto the Redis list `emails_sent`.
require "sidekiq"

Sidekiq.configure_client { |c| c.redis = { url: ENV.fetch("REDIS_URL") } }
Sidekiq.configure_server { |c| c.redis = { url: ENV.fetch("REDIS_URL") } }

class TrialExpiredJob
  include Sidekiq::Job

  def perform(email)
    Sidekiq.redis { |r| r.call("RPUSH", "emails_sent", "trial_expired:#{email}:#{Time.now.utc.iso8601}") }
  end
end
