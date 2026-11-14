"""Seed two monthly subscriptions with Polar's own test factories (tests/fixtures).

A: $10/month, canceled by the customer, set to end at the period end.
B: free ($0)/month, active, renews.
Prints A's id. B's customer email is free-<run>@example.com.
"""
import asyncio
import sys

sys.path.insert(0, "/app/server")  # the server checkout, not the script's folder

from polar.kit.db.postgres import create_async_sessionmaker
from polar.models.subscription import SubscriptionRecurringInterval
from polar.postgres import create_async_engine
from tests.fixtures.database import save_fixture_factory
from tests.fixtures.random_objects import (
    create_account,
    create_active_subscription,
    create_customer,
    create_organization,
    create_product,
    create_user,
)


async def main(run: str) -> None:
    engine = create_async_engine("script")
    async with create_async_sessionmaker(engine)() as session:
        save = save_fixture_factory(session)
        user = await create_user(save)
        account = await create_account(save, user)
        org = await create_organization(save, account, name_prefix=f"cs{run}")
        month = SubscriptionRecurringInterval.month

        paid = await create_product(save, organization=org, recurring_interval=month, name="Pro")
        alice = await create_customer(save, organization=org, email=f"cancel-{run}@example.com")
        a = await create_active_subscription(save, product=paid, customer=alice, cancel_at_period_end=True)

        free = await create_product(save, organization=org, recurring_interval=month, name="Free", prices=[(None, "usd")])
        bob = await create_customer(save, organization=org, email=f"free-{run}@example.com")
        await create_active_subscription(save, product=free, customer=bob)

        await session.commit()
        print(a.id)
    await engine.dispose()


asyncio.run(main(sys.argv[1]))
