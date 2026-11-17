# Tone of voice (Arabic)

Every message the bot shows lives in `src/texts.rs` (the rest of the code reaches it as `i18n`). A new string goes
there, never inline in a command. The bot speaks like a friendly member of the server, not like a form.

## Rules

1. **Plain, light Arabic.** Modern Standard Arabic that anyone understands; no stiff bureaucratic words
   ("تعذّر"، "يرجى"، "تم إجراء") where a simple one exists ("لم أستطع"، "جرّب"، "حدّثتُ").
2. **First person.** The bot says "أضفتُ"، "أخفيتُ"، "لم أجد" — not "تمت الإضافة" in the passive.
3. **Say what to do next.** An error is a short reason and a way out: "لم أجد شيئًا بهذا الاسم. جرّب كتابة
   مختلفة أو الاسم بالإنجليزية."
4. **Short.** One or two sentences. Details go in the embed, not in a notice.
5. **No blame.** "هذه الرسالة لمن استخدم الأمر" instead of "ليست لك".
6. **One light emoji at most**, only where it adds warmth (✅ 🔔 🎨 😉). Never a row of them.
7. **Command names stay English** (`/anime`, `/top`) because Discord needs them; everything the member reads is Arabic.
8. **Words for things are fixed:** عمل (work), شخصية (character), بروفايل, ذكّرني (remind me), فعالية (event),
   الحالة (status). Don't invent a second word for the same thing.
9. **Credits stay exact.** The TMDB statement, "Streaming data by JustWatch", "Data from IGDB.com" and similar are
   required wording and are never rephrased.

## Before / after

| Before | After |
| --- | --- |
| حدث خطأ غير متوقع، حاول مرة أخرى لاحقًا. | عذرًا، حدث خلل من جهتي. جرّب مرة أخرى بعد قليل. |
| لم أجد نتائج مطابقة. | لم أجد شيئًا بهذا الاسم. جرّب كتابة مختلفة أو الاسم بالإنجليزية. |
| هذه الأزرار ليست لك. | هذه الرسالة لمن استخدم الأمر. شغّل الأمر بنفسك لتتحكم بها. |
| لا توجد شخصيات في هذا البروفايل بعد. | بروفايلك فارغ حتى الآن. ابحث عن شخصية تحبها وأضفها بزر «أضف لبروفايلي». |
| تمت إضافة **X** في المركز 3 ✅ | أضفتُ **X** إلى بروفايلك في المركز 3 ✅ |
| تم الإلغاء. | تمام، ألغيتُ العملية. |
| تم تحديث الغلاف. | غلافك الجديد جاهز 🎨 |
| تم إخفاء عناصر غير مناسبة. | أخفيتُ بعض النتائج لأنها غير مناسبة. |
| 🔔 سأذكّرك عند صدور حلقة جديدة (رسالة خاصة). | تمام 🔔 سأرسل لك رسالة خاصة عند صدور الجديد. |
| توجد لعبة جارية في هذه القناة. انتظر انتهاءها. | هناك فعالية جارية في هذه القناة الآن. انتظر حتى تنتهي. |

## Checks

`texts::tests` fails when a message has no Arabic in it (outside a short list of brand names), when it is empty, or
when it starts with the stiff passive "تم" followed by a verb noun that has a first-person form in the table above.
