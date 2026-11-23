-- 0003: student status and last course (D-95, D-96; BR-STU-03, BR-STU-06).
-- * ACTIVO: a course enrollment that is not BAJA, in a calendar row with status Confirmado (D-95). Enrollment status
--   is compared with lower() (OD-04-02).
-- * last_course_date: the first class date of the latest course enrollment whose calendar row is Confirmado or
--   Completado (D-96); Pendiente and Cancelado rows do not count.
-- Only the view changes; no table, no data. v_student_certification_progress uses neither value and stays as it is.
BEGIN;

DROP VIEW IF EXISTS v_student_academic;

CREATE VIEW v_student_academic AS
SELECT s.uuid AS student_uuid,
       CASE WHEN a.cust_type = 'ACTIVO' THEN 1 ELSE 0 END AS is_affiliate,
       CASE WHEN EXISTS (SELECT 1 FROM enrollments_course e JOIN calendar_course cc ON cc.uuid = e.course_calendar_uuid
                          WHERE e.student_uuid = s.uuid AND cc.status = 'Confirmado'
                            AND lower(e.status) <> 'baja') THEN 1 ELSE 0 END AS is_active,
       CASE WHEN EXISTS (SELECT 1 FROM enrollments_course e JOIN calendar_course cc ON cc.uuid = e.course_calendar_uuid
                          WHERE e.student_uuid = s.uuid AND cc.status = 'Confirmado'
                            AND lower(e.status) <> 'baja') THEN 'ACTIVO' ELSE 'INACTIVO' END AS status,
       (SELECT ROUND(AVG(e.puntuacion)) FROM enrollments_course e WHERE e.student_uuid = s.uuid AND e.puntuacion > 69) AS average_points,
       (SELECT COALESCE(SUM(c.credits), 0) FROM enrollments_course e JOIN courses c ON c.uuid = e.course_uuid
         WHERE e.student_uuid = s.uuid AND e.puntuacion > 69) AS total_credits,
       (SELECT COALESCE(SUM(c.hours), 0) FROM enrollments_course e JOIN courses c ON c.uuid = e.course_uuid
         WHERE e.student_uuid = s.uuid AND e.puntuacion > 69) AS total_hours,
       (SELECT MAX(l.course_date) FROM v_enrollments_course_lines l
         WHERE l.student_uuid = s.uuid AND l.course_status IN ('Confirmado', 'Completado')) AS last_course_date
FROM students s
LEFT JOIN affiliates a ON a.uuid = s.member_uuid;

COMMIT;
