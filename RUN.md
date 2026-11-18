# Running anpot (static Linux binary)

1. Unzip `anpot-linux.zip` and upload `anpot` to your host.
2. Create a file named `.env` next to it (use `.env` from the repository as the template) with at least:
   ```
   DISCORD_TOKEN=your-bot-token
   DATABASE_URL=sqlite://bot.db?mode=rwc
   ```
3. Make it executable and start it: `chmod +x anpot && ./anpot`
4. Back up `bot.db` regularly. Nothing else is needed: migrations and fonts are inside the binary.

# التشغيل (بالعربية)

1. فك ضغط `anpot-linux.zip` وارفع الملف `anpot` إلى الاستضافة.
2. أنشئ ملف `.env` بجانبه (استخدم `.env` الموجود في المستودع كقالب) وضع فيه `DISCORD_TOKEN` و `DATABASE_URL`.
3. اجعله قابلًا للتنفيذ ثم شغّله: `chmod +x anpot && ./anpot`
4. خذ نسخة احتياطية من `bot.db` بانتظام. لا حاجة لأي ملفات أخرى.
