"""Create a workspace, a project with auto-close and auto-archive after 1 month, and one started issue."""
import os, sys
sys.path.insert(0, "/code")
os.environ.setdefault("DJANGO_SETTINGS_MODULE", "plane.settings.production")
import django
django.setup()
from plane.db.models import User, Workspace, Project, State, Issue

run = sys.argv[1]
user = User.objects.create(email=f"cs-{run}@example.com", username=f"cs{run}")
ws = Workspace.objects.create(name=f"cs {run}", slug=f"cs-{run}", owner=user, created_by=user)
project = Project.objects.create(name=f"cs {run}", identifier="CS", workspace=ws, created_by=user,
                                 close_in=1, archive_in=1)
started = State.objects.create(name="In Progress", group="started", project=project, workspace=ws, default=True)
cancelled = State.objects.create(name="Cancelled", group="cancelled", project=project, workspace=ws)
project.default_state = cancelled  # the state auto-close moves issues to
project.save()
issue = Issue.objects.create(name="Stale issue", project=project, workspace=ws, state=started, created_by=user)
print(issue.id)
