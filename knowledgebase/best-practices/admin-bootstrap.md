# Admin Bootstrap

To promote a user to the **Admin** role (required to manage teams, tasks, and approvals), execute the following SQL in the Supabase Dashboard SQL Editor:

```sql
UPDATE public.profiles 
SET role = 'Admin' 
WHERE email = 'your-admin-email@example.com';
```

## Initial Setup Note
If you have just signed up and your profile was created automatically by the `on_auth_user_created` trigger, your default role will be `Apprentice`. Use the command above to gain access to the `/admin` dashboard.
