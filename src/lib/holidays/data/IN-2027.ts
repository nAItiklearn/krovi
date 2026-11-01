// India — 2027 central-government holidays (Delhi / New Delhi administrative offices).
//
// PRIMARY SOURCE (official; text extracted from the PDF and checked against the
// DoPT website record):
//   Government of India, Ministry of Personnel, Public Grievances and Pensions,
//   Department of Personnel and Training (DoPT), Pers. Policy (JCA) Section.
//   Office Memorandum F.No.12/2/2023-JCA dated 16 July 2026,
//   "Holidays to be observed in Central Government Offices during the year 2027".
//   Annexure-I = holidays (17), Annexure-II = restricted holidays (32).
//   PDF:    https://master-dopt.digifootprint.gov.in/static/uploads/2026/08/2ffd159748162a48c025f7e6ca5807d5.pdf
//   Record: https://cms-dopt.digifootprint.gov.in/central_documents/holidays-to-be-observed-in-central-government-offices-during-the-year-2027-regarding/
// RETRIEVED: 2026-11-01.
import type { YearData } from '../types';

export const IN_2027: YearData = {
  region: 'IN',
  year: 2027,
  source: {
    authority: 'Government of India, Department of Personnel & Training (DoPT)',
    title: 'Holidays to be observed in Central Government Offices during the year 2027',
    reference: 'Office Memorandum F.No.12/2/2023-JCA dated 16 July 2026 (Annexure-I and Annexure-II)',
    url: 'https://master-dopt.digifootprint.gov.in/static/uploads/2026/08/2ffd159748162a48c025f7e6ca5807d5.pdf',
    supplements: [],
    retrieved: '2026-11-01',
    crossChecks: [
      'People Matters (26 Oct 2026) "Central Government holiday list 2027": its gazetted list matches the OM, including Diwali on Friday 29 October 2027.',
      'OM text transcribed by Staff News (staffnews.in, 18 Jul 2026, "List of Holidays 2027 ... O.M. dated 16.07.2026").',
      'National Portal of India holiday calendar (india.gov.in/calendar, NIC; getCentralHolidays year=2027): all 32 restricted dates and 16 of the 17 gazetted dates match the OM.',
    ],
    discrepancies: [
      'The National Portal\'s 2027 central list has only 16 gazetted holidays: it omits Diwali (Deepavali), Friday 29 October 2027, although it lists Naraka Chaturdasi (28 Oct), Govardhan Puja (30 Oct) and Bhai Duj (31 Oct). The OM Annexure-I (item 15) and para 6 both give Diwali on 29 October 2027. Resolved in favour of the OM: Diwali is included.',
    ],
    notes: [
      'Scope: administrative offices of the Central Government in Delhi / New Delhi. State governments, banks and offices outside Delhi follow their own lists.',
      'Id-ul-Fitr, Id-ul-Zuha, Muharram and Milad-un-Nabi depend on moon sighting and may be shifted by a later DoPT order (OM para 5); in 2026 Id-ul-Zuha was shifted by a day. Re-check these four dates when 2027 approaches.',
      'DoPT declares the Ambedkar Jayanti (14 April) closed holiday by a separate order shortly before the date (it did so in 2024, 2025 and 2026); no such order exists for 2027 yet, so it is not included.',
      '15 August 2027 carries both Independence Day and Milad-un-Nabi (a Sunday); OM para 3.2 allows no substitute holiday.',
    ],
  },
  holidays: [
    // ---- Annexure-I: holidays (gazetted / closed) ----
    { date: '2027-01-26', name: 'Republic Day', type: 'gazetted' },
    { date: '2027-03-10', name: 'Id-ul-Fitr (Eid)', type: 'gazetted' },
    { date: '2027-03-23', name: 'Holi', type: 'gazetted' },
    { date: '2027-03-26', name: 'Good Friday', type: 'gazetted' },
    { date: '2027-04-15', name: 'Ram Navami', type: 'gazetted' },
    { date: '2027-04-19', name: 'Mahavir Jayanti', type: 'gazetted' },
    { date: '2027-05-17', name: 'Id-ul-Zuha (Bakrid)', type: 'gazetted' },
    { date: '2027-05-20', name: 'Buddha Purnima', type: 'gazetted' },
    { date: '2027-06-16', name: 'Muharram', type: 'gazetted' },
    { date: '2027-08-15', name: 'Independence Day', type: 'gazetted' },
    { date: '2027-08-15', name: 'Milad-un-Nabi (Id-e-Milad)', type: 'gazetted' },
    { date: '2027-08-25', name: 'Janmashtami (Vaishnava)', type: 'gazetted' },
    { date: '2027-10-02', name: "Mahatma Gandhi's Birthday", type: 'gazetted' },
    { date: '2027-10-09', name: 'Dussehra (Vijaya Dashmi)', type: 'gazetted' },
    { date: '2027-10-29', name: 'Diwali (Deepavali)', type: 'gazetted' },
    { date: '2027-11-14', name: "Guru Nanak's Birthday", type: 'gazetted' },
    { date: '2027-12-25', name: 'Christmas Day', type: 'gazetted' },

    // ---- Annexure-II: restricted holidays (employee may take any two) ----
    { date: '2027-01-01', name: "New Year's Day", type: 'restricted' },
    { date: '2027-01-14', name: 'Makar Sankranti / Magha Bihu', type: 'restricted' },
    { date: '2027-01-15', name: "Pongal / Guru Gobind Singh's Birthday", type: 'restricted' },
    { date: '2027-02-11', name: 'Sri Panchami / Basant Panchami', type: 'restricted' },
    { date: '2027-02-19', name: 'Shiva Ji Jayanti', type: 'restricted' },
    { date: '2027-02-20', name: "Guru Ravi Das's Birthday", type: 'restricted' },
    { date: '2027-03-02', name: "Swami Dayananda Saraswati's Birthday", type: 'restricted' },
    { date: '2027-03-05', name: 'Jamat-Ul-Vida', type: 'restricted' },
    { date: '2027-03-06', name: 'Maha Shivratri', type: 'restricted' },
    { date: '2027-03-22', name: 'Holika Dahan / Dolyatra', type: 'restricted' },
    { date: '2027-03-28', name: 'Easter Sunday', type: 'restricted' },
    {
      date: '2027-04-07',
      name: 'Chaitra Sukladi / Gudi Padava / Ugadi / Cheti Chand / 1st Navratra',
      type: 'restricted',
    },
    { date: '2027-04-14', name: "Vaisakhi / Vishu / Meshadi (Tamil New Year's Day)", type: 'restricted' },
    { date: '2027-04-15', name: 'Vaisakhadi (Bengal) / Bahag Bihu (Assam)', type: 'restricted' },
    { date: '2027-05-09', name: "Guru Rabindranath Tagore's Birthday", type: 'restricted' },
    { date: '2027-07-05', name: 'Rath Yatra', type: 'restricted' },
    { date: '2027-08-15', name: "Parsi New Year's Day / Nauroz", type: 'restricted' },
    { date: '2027-08-17', name: 'Raksha Bandhan', type: 'restricted' },
    { date: '2027-09-04', name: 'Ganesh Chaturthi / Vinayak Chaturthi', type: 'restricted' },
    { date: '2027-09-12', name: 'Onam or Thiru Onam Day', type: 'restricted' },
    { date: '2027-10-06', name: 'Dussehra (Saptami)', type: 'restricted' },
    { date: '2027-10-07', name: 'Dussehra (Mahashtami)', type: 'restricted' },
    { date: '2027-10-08', name: 'Dussehra (Mahanavmi)', type: 'restricted' },
    { date: '2027-10-15', name: "Maharishi Valmiki's Birthday", type: 'restricted' },
    { date: '2027-10-18', name: 'Karaka Chaturthi (Karwa Chouth)', type: 'restricted' },
    { date: '2027-10-28', name: 'Naraka Chaturdasi', type: 'restricted' },
    { date: '2027-10-30', name: 'Govardhan Puja', type: 'restricted' },
    { date: '2027-10-31', name: 'Bhai Duj', type: 'restricted' },
    { date: '2027-11-04', name: 'Pratihar Shashthi or Surya Shashthi (Chhat Puja)', type: 'restricted' },
    { date: '2027-11-24', name: "Guru Teg Bahadur's Martyrdom Day", type: 'restricted' },
    { date: '2027-12-12', name: "Hazarat Ali's Birthday", type: 'restricted' },
    { date: '2027-12-24', name: 'Christmas Eve', type: 'restricted' },
  ],
};
