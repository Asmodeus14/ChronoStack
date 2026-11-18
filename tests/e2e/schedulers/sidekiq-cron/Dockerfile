# SPDX-License-Identifier: MIT OR Apache-2.0
FROM ruby:3.3
RUN gem install --no-document sidekiq -v 8.1.7 && gem install --no-document sidekiq-cron -v 2.3.1
WORKDIR /app
COPY worker.rb ./
ENV REDIS_URL=redis://redis:6379/0
CMD ["sidekiq", "-r", "./worker.rb", "-c", "2"]
