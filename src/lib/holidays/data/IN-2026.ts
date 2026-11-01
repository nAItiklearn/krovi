// India — 2026 central-government holidays (Delhi / New Delhi administrative offices).
//
// PRIMARY SOURCE (official, transcribed by hand from the scanned PDF):
//   Government of India, Ministry of Personnel, Public Grievances and Pensions,
//   Department of Personnel and Training (DoPT), Establishment (JCA) Section.
//   Office Memorandum F.No.12/2/2023-JCA dated 3 July 2025,
//   "Holidays to be observed in Central Government Offices during the year 2026".
//   Annexure-I = holidays (17), Annexure-II = restricted holidays (34).
//   https://documents.doptcirculars.nic.in/D2/D02est/Holidays%20to%20be%20observed%20in%20Central%20Government%20Offices%20during%20the%20year%2020265vgBs.pdf
// SUPPLEMENTS:
//   * DoPT OM F.No.12/3/2023-JCA dated 22 May 2026: Id-ul-Zuha (Bakrid) moved from
//     27 May to 28 May 2026 for Delhi / New Delhi offices (moon sighting).
//   * DoPT OM F.No.12/4/2020-JCA dated 9 April 2026: Tuesday 14 April 2026
//     (Birthday of Dr. B.R. Ambedkar) declared a holiday for all central government offices.
// RETRIEVED: 2026-11-01.
import type { YearData } from '../types';

export const IN_2026: YearData = {
  region: 'IN',
  year: 2026,
  source: {
    authority: 'Government of India, Department of Personnel & Training (DoPT)',
    title: 'Holidays to be observed in Central Government Offices during the year 2026',
    reference:
      'Office Memorandum F.No.12/2/2023-JCA dated 3 July 2025 (Annexure-I and Annexure-II), as amended by OMs dated 22 May 2026 and 9 April 2026',
    url: 'https://documents.doptcirculars.nic.in/D2/D02est/Holidays%20to%20be%20observed%20in%20Central%20Government%20Offices%20during%20the%20year%2020265vgBs.pdf',
    supplements: [
      {
        reference:
          'OM F.No.12/3/2023-JCA dated 22 May 2026: Id-ul-Zuha (Bakrid) shifted to 28 May 2026 in place of 27 May 2026',
        url: 'https://www.staffnews.in/wp-content/uploads/2026/05/Change-in-the-date-of-holiday-of-Id-ul-Zuhamg709.pdf',
      },
      {
        reference:
          'OM F.No.12/4/2020-JCA dated 9 April 2026: holiday on 14 April 2026 (Birthday of Dr. B.R. Ambedkar)',
        url: 'https://www.staffnews.in/2026/04/declaration-of-holiday-on-14th-april-2026.html',
      },
    ],
    retrieved: '2026-11-01',
    crossChecks: [
      'National Portal of India holiday calendar (india.gov.in/calendar, NIC; getCentralHolidays year=2026): all 17 gazetted dates match the OM as amended (the portal already shows Id-ul-Zuha on 28 May); its 32 restricted rows match the OM\'s 34 rows once the two merged pairs are expanded.',
      'Id-ul-Zuha shift to 28 May 2026: Livemint, NDTV Profit and People Matters (all 22 May 2026) reporting the Centre\'s order; DoPT OM text transcribed by Staff News.',
      'Ambedkar Jayanti 2026 holiday: DoPT OM of 9 Apr 2026 as transcribed by Staff News; NammaWard (12 Apr 2026) and APAC Media (10 Apr 2026) report "Centre announces April 14 holiday ... for all Central Govt offices".',
    ],
    discrepancies: [
      'OM Annexure-I gives Id-ul-Zuha as 27 May 2026; the OM of 22 May 2026 (and the National Portal) move it to 28 May. Resolved: 28 May is used.',
      'A Deccan Herald article (28 Nov 2025) embeds a table of the 17 holidays in which 9 dates are wrong (for example Id-ul-Fitr 29 Mar, Ram Navami 31 Mar, Buddha Purnima 31 May, Milad-un-Nabi 24 Sep, Diwali 21 Nov, Guru Nanak 29 Nov). It contradicts the OM text itself (para 6: Diwali is Sunday 8 November 2026). Ignored.',
      'The National Portal merges Makar Sankranti with Magha Bihu / Pongal (14 Jan) and Holika Dahan with Dolyatra (3 Mar) into single rows, and does not show the 14 Apr 2026 Ambedkar Jayanti closed holiday. Resolved in favour of the DoPT documents: the OM\'s 34 restricted rows are kept as separate entries and Ambedkar Jayanti is included as gazetted.',
    ],
    notes: [
      'Scope: administrative offices of the Central Government in Delhi / New Delhi. State governments, banks and offices outside Delhi follow their own lists.',
      'No other 2026 change order was found; Id-ul-Fitr (21 Mar), Muharram (26 Jun) and Milad-un-Nabi (26 Aug) are as notified in the OM and still shown unchanged on the National Portal.',
    ],
  },
  holidays: [
    // ---- Annexure-I: holidays (gazetted / closed) ----
    { date: '2026-01-26', name: 'Republic Day', type: 'gazetted' },
    { date: '2026-03-04', name: 'Holi', type: 'gazetted' },
    { date: '2026-03-21', name: 'Id-ul-Fitr (Eid)', type: 'gazetted' },
    { date: '2026-03-26', name: 'Ram Navami', type: 'gazetted' },
    { date: '2026-03-31', name: 'Mahavir Jayanti', type: 'gazetted' },
    { date: '2026-04-03', name: 'Good Friday', type: 'gazetted' },
    // Supplementary OM dated 9 Apr 2026 (not part of Annexure-I).
    { date: '2026-04-14', name: "Dr. B.R. Ambedkar's Birthday (Ambedkar Jayanti)", type: 'gazetted' },
    { date: '2026-05-01', name: 'Buddha Purnima', type: 'gazetted' },
    // Annexure-I says 27 May; moved to 28 May by OM dated 22 May 2026.
    { date: '2026-05-28', name: 'Id-ul-Zuha (Bakrid)', type: 'gazetted' },
    { date: '2026-06-26', name: 'Muharram', type: 'gazetted' },
    { date: '2026-08-15', name: 'Independence Day', type: 'gazetted' },
    { date: '2026-08-26', name: 'Milad-un-Nabi (Id-e-Milad)', type: 'gazetted' },
    { date: '2026-09-04', name: 'Janmashtami (Vaishnava)', type: 'gazetted' },
    { date: '2026-10-02', name: "Mahatma Gandhi's Birthday", type: 'gazetted' },
    { date: '2026-10-20', name: 'Dussehra (Vijaya Dashmi)', type: 'gazetted' },
    { date: '2026-11-08', name: 'Diwali (Deepavali)', type: 'gazetted' },
    { date: '2026-11-24', name: "Guru Nanak's Birthday", type: 'gazetted' },
    { date: '2026-12-25', name: 'Christmas Day', type: 'gazetted' },

    // ---- Annexure-II: restricted holidays (employee may take any two) ----
    { date: '2026-01-01', name: "New Year's Day", type: 'restricted' },
    { date: '2026-01-03', name: "Hazarat Ali's Birthday", type: 'restricted' },
    { date: '2026-01-14', name: 'Makar Sankranti', type: 'restricted' },
    { date: '2026-01-14', name: 'Magha Bihu / Pongal', type: 'restricted' },
    { date: '2026-01-23', name: 'Sri Panchami / Basant Panchami', type: 'restricted' },
    { date: '2026-02-01', name: "Guru Ravi Das's Birthday", type: 'restricted' },
    { date: '2026-02-12', name: 'Birthday of Swami Dayananda Saraswati', type: 'restricted' },
    { date: '2026-02-15', name: 'Maha Shivratri', type: 'restricted' },
    { date: '2026-02-19', name: 'Shiva ji Jayanti', type: 'restricted' },
    { date: '2026-03-03', name: 'Holika Dahan', type: 'restricted' },
    { date: '2026-03-03', name: 'Dolyatra', type: 'restricted' },
    { date: '2026-03-19', name: 'Chaitra Sukladi / Gudi Padava / Ugadi / Cheti Chand', type: 'restricted' },
    { date: '2026-03-20', name: 'Jamat-Ul-Vida', type: 'restricted' },
    { date: '2026-04-05', name: 'Easter Sunday', type: 'restricted' },
    { date: '2026-04-14', name: "Vaisakhi / Vishu / Meshadi (Tamil New Year's Day)", type: 'restricted' },
    { date: '2026-04-15', name: 'Vaisakhadi (Bengal) / Bahag Bihu (Assam)', type: 'restricted' },
    { date: '2026-05-09', name: 'Birthday of Guru Rabindranath Tagore', type: 'restricted' },
    { date: '2026-07-16', name: 'Rath Yatra', type: 'restricted' },
    { date: '2026-08-15', name: "Parsi New Year's Day / Nauraj", type: 'restricted' },
    { date: '2026-08-26', name: 'Onam or Thiru Onam Day', type: 'restricted' },
    { date: '2026-08-28', name: 'Raksha Bandhan', type: 'restricted' },
    { date: '2026-09-14', name: 'Ganesh Chaturthi / Vinayak Chaturthi', type: 'restricted' },
    { date: '2026-10-18', name: 'Dussehra (Saptami)', type: 'restricted' },
    { date: '2026-10-19', name: 'Dussehra (Mahashtami)', type: 'restricted' },
    { date: '2026-10-20', name: 'Dussehra (Mahanavmi)', type: 'restricted' },
    { date: '2026-10-26', name: "Maharishi Valmiki's Birthday", type: 'restricted' },
    { date: '2026-10-29', name: 'Karaka Chaturthi (Karwa Chouth)', type: 'restricted' },
    { date: '2026-11-08', name: 'Naraka Chaturdasi', type: 'restricted' },
    { date: '2026-11-09', name: 'Govardhan Puja', type: 'restricted' },
    { date: '2026-11-11', name: 'Bhai Duj', type: 'restricted' },
    { date: '2026-11-15', name: 'Pratihar Shashthi or Surya Shashthi (Chhat Puja)', type: 'restricted' },
    { date: '2026-11-24', name: "Guru Teg Bahadur's Martyrdom Day", type: 'restricted' },
    { date: '2026-12-23', name: "Hazarat Ali's Birthday", type: 'restricted' },
    { date: '2026-12-24', name: 'Christmas Eve', type: 'restricted' },
  ],
};
