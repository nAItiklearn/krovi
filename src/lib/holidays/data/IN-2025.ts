// India — 2025 central-government holidays (Delhi / New Delhi administrative offices).
//
// PRIMARY SOURCE (official, transcribed by hand from the scanned PDF):
//   Government of India, Ministry of Personnel, Public Grievances and Pensions,
//   Department of Personnel and Training (DoPT), Establishment (JCA) Section.
//   Office Memorandum F.No.12/2/2023-JCA dated 9 July 2024,
//   "Holidays to be observed in Central Government Offices during the year 2025".
//   Annexure-I = holidays (17), Annexure-II = restricted holidays (34).
//   https://documents.doptcirculars.nic.in/D2/D02est/List%20of%20holidays%202025KtKLw.pdf
// SUPPLEMENT:
//   DoPT OM F.No.12/4/2020-JCA2 dated 27 March 2025 declaring Monday 14 April 2025
//   (Birthday of Dr. B.R. Ambedkar) a holiday for all central government offices.
// RETRIEVED: 2026-11-01.
import type { YearData } from '../types';

export const IN_2025: YearData = {
  region: 'IN',
  year: 2025,
  source: {
    authority: 'Government of India, Department of Personnel & Training (DoPT)',
    title: 'Holidays to be observed in Central Government Offices during the year 2025',
    reference: 'Office Memorandum F.No.12/2/2023-JCA dated 9 July 2024 (Annexure-I and Annexure-II)',
    url: 'https://documents.doptcirculars.nic.in/D2/D02est/List%20of%20holidays%202025KtKLw.pdf',
    supplements: [
      {
        reference:
          'OM F.No.12/4/2020-JCA2 dated 27 March 2025: holiday on 14 April 2025 (Birthday of Dr. B.R. Ambedkar)',
        url: 'https://documents.doptcirculars.nic.in/D2/D02est/Holiday%20on%2014th%20April%202025%20Birthday%20of%20Dr%20B%20R%20AmbedkarDHZ8b.pdf',
      },
    ],
    retrieved: '2026-11-01',
    crossChecks: [
      'National Portal of India holiday calendar (india.gov.in/calendar, NIC; getCentralHolidays year=2025): all 17 gazetted and all 34 restricted dates are identical to the OM.',
      'Ambedkar Jayanti 2025 holiday: Livemint (28 Mar 2025) "Centre declares holiday for BR Ambedkar\'s birth anniversary on April 14"; OM text transcribed by Staff News (staffnews.in/2025/03/declaration-of-holiday-on-14th-april-2025.html).',
    ],
    discrepancies: [
      'The National Portal lists 14 Apr 2025 only as a restricted holiday (Meshadi); it does not show the separate OM of 27 Mar 2025 that made 14 Apr 2025 a closed holiday. Resolved in favour of the DoPT OM: Ambedkar Jayanti is included as gazetted.',
    ],
    notes: [
      'Scope: administrative offices of the Central Government in Delhi / New Delhi. State governments, banks and offices outside Delhi follow their own lists.',
      'Id-ul-Fitr, Id-ul-Zuha, Muharram and Milad-un-Nabi depend on moon sighting and can shift by a day (OM para 5). No change order for 2025 was found.',
      'Annexure-I has 17 entries; 2 Oct carries both Mahatma Gandhi\'s Birthday and Dussehra (OM para 3.2: no substitute holiday).',
    ],
  },
  holidays: [
    // ---- Annexure-I: holidays (gazetted / closed) ----
    { date: '2025-01-26', name: 'Republic Day', type: 'gazetted' },
    { date: '2025-02-26', name: 'Maha Shivaratri', type: 'gazetted' },
    { date: '2025-03-14', name: 'Holi', type: 'gazetted' },
    { date: '2025-03-31', name: 'Id-ul-Fitr (Eid)', type: 'gazetted' },
    { date: '2025-04-10', name: 'Mahavir Jayanti', type: 'gazetted' },
    // Supplementary OM dated 27 Mar 2025 (not part of Annexure-I).
    { date: '2025-04-14', name: "Dr. B.R. Ambedkar's Birthday (Ambedkar Jayanti)", type: 'gazetted' },
    { date: '2025-04-18', name: 'Good Friday', type: 'gazetted' },
    { date: '2025-05-12', name: 'Buddha Purnima', type: 'gazetted' },
    { date: '2025-06-07', name: 'Id-ul-Zuha (Bakrid)', type: 'gazetted' },
    { date: '2025-07-06', name: 'Muharram', type: 'gazetted' },
    { date: '2025-08-15', name: 'Independence Day', type: 'gazetted' },
    { date: '2025-08-16', name: 'Janmashtami', type: 'gazetted' },
    { date: '2025-09-05', name: 'Milad-un-Nabi (Id-e-Milad)', type: 'gazetted' },
    { date: '2025-10-02', name: "Mahatma Gandhi's Birthday", type: 'gazetted' },
    { date: '2025-10-02', name: 'Dussehra (Vijaya Dashmi)', type: 'gazetted' },
    { date: '2025-10-20', name: 'Diwali (Deepavali)', type: 'gazetted' },
    { date: '2025-11-05', name: "Guru Nanak's Birthday", type: 'gazetted' },
    { date: '2025-12-25', name: 'Christmas Day', type: 'gazetted' },

    // ---- Annexure-II: restricted holidays (employee may take any two) ----
    { date: '2025-01-01', name: "New Year's Day", type: 'restricted' },
    { date: '2025-01-06', name: "Guru Gobind Singh's Birthday", type: 'restricted' },
    {
      date: '2025-01-14',
      name: "Makar Sankranti / Magha Bihu / Pongal / Hazarat Ali's Birthday",
      type: 'restricted',
    },
    { date: '2025-02-02', name: 'Basant Panchami / Sri Panchami', type: 'restricted' },
    { date: '2025-02-12', name: "Guru Ravi Das's Birthday", type: 'restricted' },
    { date: '2025-02-19', name: 'Shiva ji Jayanti', type: 'restricted' },
    { date: '2025-02-23', name: 'Birthday of Swami Dayananda Saraswati', type: 'restricted' },
    { date: '2025-03-13', name: 'Holika Dahan', type: 'restricted' },
    { date: '2025-03-14', name: 'Dolyatra', type: 'restricted' },
    { date: '2025-03-28', name: 'Jamat-Ul-Vida', type: 'restricted' },
    { date: '2025-03-30', name: 'Chaitra Sukladi / Gudi Padava / Ugadi / Cheti Chand', type: 'restricted' },
    { date: '2025-04-06', name: 'Ram Navmi', type: 'restricted' },
    { date: '2025-04-13', name: 'Vaisakhi / Vishu', type: 'restricted' },
    { date: '2025-04-14', name: "Meshadi (Tamil New Year's Day)", type: 'restricted' },
    { date: '2025-04-15', name: 'Vaisakhadi (Bengal) / Bahag Bihu (Assam)', type: 'restricted' },
    { date: '2025-04-20', name: 'Easter Sunday', type: 'restricted' },
    { date: '2025-05-09', name: 'Birthday of Guru Rabindranath Tagore', type: 'restricted' },
    { date: '2025-06-27', name: 'Rath Yatra', type: 'restricted' },
    { date: '2025-08-09', name: 'Raksha Bandhan', type: 'restricted' },
    { date: '2025-08-15', name: "Parsi New Year's Day / Nauraj", type: 'restricted' },
    { date: '2025-08-15', name: 'Janmashtami (Smarta)', type: 'restricted' },
    { date: '2025-08-27', name: 'Ganesh Chaturthi / Vinayaka Chaturthi', type: 'restricted' },
    { date: '2025-09-05', name: 'Onam or Thiru Onam Day', type: 'restricted' },
    { date: '2025-09-29', name: 'Dussehra (Saptami)', type: 'restricted' },
    { date: '2025-09-30', name: 'Dussehra (Mahashtami)', type: 'restricted' },
    { date: '2025-10-01', name: 'Dussehra (Mahanavmi)', type: 'restricted' },
    { date: '2025-10-07', name: "Maharishi Valmiki's Birthday", type: 'restricted' },
    { date: '2025-10-10', name: 'Karaka Chaturthi (Karwa Chouth)', type: 'restricted' },
    { date: '2025-10-20', name: 'Naraka Chaturdasi', type: 'restricted' },
    { date: '2025-10-22', name: 'Govardhan Puja', type: 'restricted' },
    { date: '2025-10-23', name: 'Bhai Duj', type: 'restricted' },
    { date: '2025-10-28', name: 'Pratihar Shashthi or Surya Shashthi (Chhat Puja)', type: 'restricted' },
    { date: '2025-11-24', name: "Guru Teg Bahadur's Martyrdom Day", type: 'restricted' },
    { date: '2025-12-24', name: 'Christmas Eve', type: 'restricted' },
  ],
};
