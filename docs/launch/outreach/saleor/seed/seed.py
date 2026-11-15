"""Three channels whose unpaid orders expire after 60 min, 24 h (1440 min) and
48 h (2880 min), one unconfirmed order in each, and a gift card that expires
in 3 days. Prints nothing; rows are found by the run id."""
import datetime
import os
import sys

sys.path.insert(0, "/app")
os.environ.setdefault("DJANGO_SETTINGS_MODULE", "saleor.settings")
import django

django.setup()
from django.utils import timezone

from saleor.channel.models import Channel
from saleor.giftcard.models import GiftCard
from saleor.order import OrderOrigin, OrderStatus
from saleor.order.models import Order

run = sys.argv[1]
for minutes in (60, 1440, 2880):
    channel = Channel.objects.create(
        name=f"cs {run} {minutes}",
        slug=f"cs-{run}-{minutes}",
        currency_code="USD",
        default_country="US",
        expire_orders_after=minutes,
    )
    Order.objects.create(
        channel=channel,
        status=OrderStatus.UNCONFIRMED,
        origin=OrderOrigin.CHECKOUT,
        currency="USD",
        user_email=f"cs-{run}-{minutes}@example.com",
        lines_count=0,
    )
GiftCard.objects.create(
    code=f"CS{run}".upper()[:16].ljust(8, "0"),
    currency="USD",
    initial_balance_amount=10,
    current_balance_amount=10,
    expiry_date=timezone.now().date() + datetime.timedelta(days=3),
    is_active=True,
)
