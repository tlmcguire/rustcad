// C interface to the parts of OpenCascade that the Kernel trait needs.
//
// Kept small on purpose: every function here is a place where memory safety
// depends on review. Rules:
//   * No C++ exception crosses the boundary. Every entry point catches all.
//   * A function that can fail returns null (or false) and, when `error` is
//     not null, stores a message allocated with malloc. The caller frees it
//     with occt_string_free.
//   * Objects returned as pointers are owned by the caller and freed with the
//     matching occt_*_free. Passing null to a free function is allowed.
//   * Face indices are zero-based and follow TopExp::MapShapes order.
//   * Nothing writes to standard output: the worker process uses it for its protocol.
//
// This file uses facilities provided by the Open CASCADE Technology software.

#include <cstdint>
#include <cstdio>
#include <iostream>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <sstream>
#include <string>
#include <vector>

#include <BRepAdaptor_Surface.hxx>
#include <BRepCheck_Analyzer.hxx>
#include <BRepAlgoAPI_BooleanOperation.hxx>
#include <BRepBuilderAPI_Copy.hxx>
#include <BRepAlgoAPI_Common.hxx>
#include <BRepAlgoAPI_Cut.hxx>
#include <BRepAlgoAPI_Fuse.hxx>
#include <BRepGProp.hxx>
#include <BRepLProp_SLProps.hxx>
#include <BRepMesh_IncrementalMesh.hxx>
#include <BRepPrimAPI_MakeBox.hxx>
#include <BRep_Tool.hxx>
#include <BinTools.hxx>
#include <GProp_GProps.hxx>
#include <IFSelect_ReturnStatus.hxx>
#include <Interface_Static.hxx>
#include <Message.hxx>
#include <Message_Messenger.hxx>
#include <Message_PrinterOStream.hxx>
#include <Poly_Triangulation.hxx>
#include <Standard_Failure.hxx>
#include <STEPControl_Reader.hxx>
#include <STEPControl_StepModelType.hxx>
#include <STEPControl_Writer.hxx>
#include <Standard_Version.hxx>
#include <TopAbs_Orientation.hxx>
#include <TopExp.hxx>
#include <TopLoc_Location.hxx>
#include <TopTools_IndexedMapOfShape.hxx>
#include <TopTools_ListOfShape.hxx>
#include <TopoDS.hxx>
#include <TopoDS_Iterator.hxx>
#include <TopoDS_Face.hxx>
#include <TopoDS_Shape.hxx>
#include <gp_Dir.hxx>
#include <gp_Pnt.hxx>
#include <gp_Pnt2d.hxx>
#include <gp_Trsf.hxx>
#include <gp_Vec.hxx>

#ifdef _WIN32
#include <io.h>
#else
#include <unistd.h>
#endif

struct OcctShape {
  TopoDS_Shape shape;
  TopTools_IndexedMapOfShape faces;

  explicit OcctShape(const TopoDS_Shape& s) : shape(s) {
    TopExp::MapShapes(shape, TopAbs_FACE, faces);
  }
};

struct OcctHistoryEntry {
  uint32_t source_shape;   // 0 for the first operand, 1 for the second
  uint32_t source_face;
  uint32_t fate;           // 0 modified, 1 split, 2 deleted
  std::vector<uint32_t> results;
};

struct OcctHistory {
  std::vector<OcctHistoryEntry> entries;
  std::vector<uint32_t> generated;
};

struct OcctBytes {
  std::string data;
};

struct OcctMesh {
  std::vector<double> positions;      // three per vertex
  std::vector<double> normals;        // three per vertex
  std::vector<uint32_t> triangles;    // three per triangle
  std::vector<uint32_t> triangle_faces;
};

namespace {

void set_error(char** error, const std::string& message) {
  if (error == nullptr) {
    return;
  }
  char* copy = static_cast<char*>(std::malloc(message.size() + 1));
  if (copy != nullptr) {
    std::memcpy(copy, message.c_str(), message.size() + 1);
  }
  *error = copy;
}

// Runs `body`, turning any exception into an error message and `fallback`.
template <typename T, typename F>
T guarded(char** error, T fallback, F body) {
  if (error != nullptr) {
    *error = nullptr;
  }
  try {
    return body();
  } catch (const Standard_Failure& failure) {
    const char* text = failure.GetMessageString();
    set_error(error, std::string("OCCT: ") + (text != nullptr && *text != 0 ? text : "failure"));
  } catch (const std::exception& failure) {
    set_error(error, std::string("C++: ") + failure.what());
  } catch (...) {
    set_error(error, "unknown C++ exception");
  }
  return fallback;
}

void record_history(BRepAlgoAPI_BooleanOperation& op,
                    const OcctShape& input,
                    uint32_t source_shape,
                    const OcctShape& result,
                    std::vector<bool>& accounted,
                    OcctHistory& history) {
  for (int i = 1; i <= input.faces.Extent(); ++i) {
    const TopoDS_Shape& face = input.faces(i);
    OcctHistoryEntry entry;
    entry.source_shape = source_shape;
    entry.source_face = static_cast<uint32_t>(i - 1);

    auto add = [&](const TopoDS_Shape& output) {
      const int index = result.faces.FindIndex(output);
      if (index > 0) {
        const uint32_t zero_based = static_cast<uint32_t>(index - 1);
        bool seen = false;
        for (uint32_t existing : entry.results) {
          seen = seen || existing == zero_based;
        }
        if (!seen) {
          entry.results.push_back(zero_based);
          accounted[zero_based] = true;
        }
      }
    };

    if (!op.IsDeleted(face)) {
      const TopTools_ListOfShape& modified = op.Modified(face);
      if (modified.IsEmpty()) {
        add(face);  // Untouched faces pass through to the result as they are.
      } else {
        for (TopTools_ListOfShape::Iterator it(modified); it.More(); it.Next()) {
          add(it.Value());
        }
      }
    }
    entry.fate = entry.results.empty() ? 2u : (entry.results.size() > 1 ? 1u : 0u);
    history.entries.push_back(std::move(entry));
  }
}

// OCCT's default messenger prints to std::cout, and the data exchange toolkits report
// through it. Remove those printers once, before the first exchange call.
void silence_messenger() {
  static const bool done = []() {
    Message::DefaultMessenger()->RemovePrinters(STANDARD_TYPE(Message_PrinterOStream));
    return true;
  }();
  (void)done;
}

// A shape read from outside is only accepted when it is valid B-rep.
bool accept(const TopoDS_Shape& shape, const char* what, char** error) {
  if (shape.IsNull()) {
    set_error(error, std::string("OCCT: ") + what + " contains no shape");
    return false;
  }
  if (!BRepCheck_Analyzer(shape).IsValid()) {
    set_error(error, std::string("OCCT: ") + what + " contains an invalid shape");
    return false;
  }
  return true;
}

// A compound holding exactly one shape is unwrapped, so a STEP file with one solid
// comes back as that solid.
TopoDS_Shape unwrap(const TopoDS_Shape& shape) {
  if (shape.ShapeType() != TopAbs_COMPOUND) {
    return shape;
  }
  TopoDS_Iterator it(shape);
  if (!it.More()) {
    return shape;
  }
  const TopoDS_Shape first = it.Value();
  it.Next();
  return it.More() ? shape : first;
}

}  // namespace

extern "C" {

// Moves standard output out of OCCT's reach, for the worker process. Duplicates the
// current standard output, points standard output at standard error, and returns the
// duplicate: a file descriptor on Unix, an OS handle on Windows. Returns -1 on failure,
// with standard output unchanged. Anything OCCT prints from then on goes to standard error.
intptr_t occt_claim_stdout() {
  std::cout.flush();
  std::fflush(stdout);
#ifdef _WIN32
  const int saved = _dup(1);
  if (saved < 0) {
    return -1;
  }
  if (_dup2(2, 1) != 0) {
    _close(saved);
    return -1;
  }
  return _get_osfhandle(saved);
#else
  const int saved = dup(1);
  if (saved < 0) {
    return -1;
  }
  if (dup2(2, 1) < 0) {
    close(saved);
    return -1;
  }
  return saved;
#endif
}

// OCCT version this shim was compiled against, for example "7.9.3".
const char* occt_version() {
  return OCC_VERSION_COMPLETE;
}

void occt_string_free(char* text) {
  std::free(text);
}

void occt_shape_free(OcctShape* shape) {
  delete shape;
}

void occt_history_free(OcctHistory* history) {
  delete history;
}

void occt_mesh_free(OcctMesh* mesh) {
  delete mesh;
}

void occt_bytes_free(OcctBytes* bytes) {
  delete bytes;
}

// A view of the bytes, valid until occt_bytes_free.
void occt_bytes_data(const OcctBytes* bytes, const uint8_t** data, size_t* len) {
  if (data == nullptr || len == nullptr) {
    return;
  }
  *data = bytes == nullptr ? nullptr : reinterpret_cast<const uint8_t*>(bytes->data.data());
  *len = bytes == nullptr ? 0 : bytes->data.size();
}

// The shape in OCCT's binary B-rep format, without triangulation. The format also stores
// transient flags (modified, checked) that tessellation and validity checks change, so a
// fresh copy without its mesh is written: the same geometry always gives the same bytes.
OcctBytes* occt_to_brep(const OcctShape* shape, char** error) {
  return guarded<OcctBytes*>(error, nullptr, [&]() -> OcctBytes* {
    if (shape == nullptr) {
      set_error(error, "null argument");
      return nullptr;
    }
    std::ostringstream out(std::ios::out | std::ios::binary);
    BRepBuilderAPI_Copy copy(shape->shape, Standard_True, Standard_False);
    if (!copy.IsDone()) {
      set_error(error, "OCCT: B-rep copy failed");
      return nullptr;
    }
    BinTools::Write(copy.Shape(), out, Standard_False, Standard_False,
                    BinTools_FormatVersion_CURRENT);
    if (!out) {
      set_error(error, "OCCT: B-rep write failed");
      return nullptr;
    }
    std::unique_ptr<OcctBytes> bytes(new OcctBytes());
    bytes->data = out.str();
    return bytes.release();
  });
}

OcctShape* occt_from_brep(const uint8_t* data, size_t len, char** error) {
  return guarded<OcctShape*>(error, nullptr, [&]() -> OcctShape* {
    if (data == nullptr) {
      set_error(error, "null argument");
      return nullptr;
    }
    std::istringstream in(std::string(reinterpret_cast<const char*>(data), len),
                          std::ios::in | std::ios::binary);
    TopoDS_Shape shape;
    BinTools::Read(shape, in);
    // A read that ran out of data leaves the stream failed, even if a shape came back.
    if (in.fail()) {
      set_error(error, "OCCT: B-rep data is truncated or could not be read");
      return nullptr;
    }
    if (!accept(shape, "B-rep data", error)) {
      return nullptr;
    }
    return new OcctShape(shape);
  });
}

// The shape as a STEP AP214 file in millimetres. The header carries a time stamp, so two
// exports of one shape differ in that line.
OcctBytes* occt_export_step(const OcctShape* shape, char** error) {
  return guarded<OcctBytes*>(error, nullptr, [&]() -> OcctBytes* {
    if (shape == nullptr) {
      set_error(error, "null argument");
      return nullptr;
    }
    silence_messenger();
    STEPControl_Writer writer;
    Interface_Static::SetCVal("write.step.unit", "MM");
    if (writer.Transfer(shape->shape, STEPControl_AsIs) != IFSelect_RetDone) {
      set_error(error, "OCCT: STEP transfer failed");
      return nullptr;
    }
    std::ostringstream out;
    if (writer.WriteStream(out) != IFSelect_RetDone || !out) {
      set_error(error, "OCCT: STEP write failed");
      return nullptr;
    }
    std::unique_ptr<OcctBytes> bytes(new OcctBytes());
    bytes->data = out.str();
    return bytes.release();
  });
}

// Every root of a STEP file, as one shape in millimetres.
OcctShape* occt_import_step(const uint8_t* data, size_t len, char** error) {
  return guarded<OcctShape*>(error, nullptr, [&]() -> OcctShape* {
    if (data == nullptr) {
      set_error(error, "null argument");
      return nullptr;
    }
    silence_messenger();
    Interface_Static::SetCVal("xstep.cascade.unit", "MM");
    std::istringstream in(std::string(reinterpret_cast<const char*>(data), len));
    STEPControl_Reader reader;
    if (reader.ReadStream("input.step", in) != IFSelect_RetDone) {
      set_error(error, "OCCT: not a readable STEP file");
      return nullptr;
    }
    if (reader.TransferRoots() == 0 || reader.NbShapes() == 0) {
      set_error(error, "OCCT: the STEP file contains no shape");
      return nullptr;
    }
    const TopoDS_Shape shape = unwrap(reader.OneShape());
    if (!accept(shape, "the STEP file", error)) {
      return nullptr;
    }
    return new OcctShape(shape);
  });
}

// An axis-aligned box. Sizes must be positive; the caller checks.
OcctShape* occt_make_box(double ox, double oy, double oz,
                         double sx, double sy, double sz,
                         char** error) {
  return guarded<OcctShape*>(error, nullptr, [&]() -> OcctShape* {
    BRepPrimAPI_MakeBox maker(gp_Pnt(ox, oy, oz), sx, sy, sz);
    const TopoDS_Shape solid = maker.Shape();
    if (solid.IsNull()) {
      set_error(error, "OCCT: box construction produced no shape");
      return nullptr;
    }
    return new OcctShape(solid);
  });
}

// op: 0 union, 1 subtract (a minus b), 2 intersect.
// On success stores a history object in *history; the caller frees both.
OcctShape* occt_boolean(int op, const OcctShape* a, const OcctShape* b,
                        OcctHistory** history, char** error) {
  return guarded<OcctShape*>(error, nullptr, [&]() -> OcctShape* {
    if (a == nullptr || b == nullptr || history == nullptr) {
      set_error(error, "null argument");
      return nullptr;
    }
    *history = nullptr;
    std::unique_ptr<BRepAlgoAPI_BooleanOperation> algo;
    switch (op) {
      case 0: algo.reset(new BRepAlgoAPI_Fuse(a->shape, b->shape)); break;
      case 1: algo.reset(new BRepAlgoAPI_Cut(a->shape, b->shape)); break;
      case 2: algo.reset(new BRepAlgoAPI_Common(a->shape, b->shape)); break;
      default:
        set_error(error, "unknown boolean operation");
        return nullptr;
    }
    if (!algo->IsDone() || algo->HasErrors()) {
      std::ostringstream text;
      algo->DumpErrors(text);
      set_error(error, "OCCT: boolean failed: " + text.str());
      return nullptr;
    }
    const TopoDS_Shape& output = algo->Shape();
    if (output.IsNull()) {
      set_error(error, "OCCT: boolean produced no shape");
      return nullptr;
    }

    std::unique_ptr<OcctShape> result(new OcctShape(output));
    std::unique_ptr<OcctHistory> record(new OcctHistory());
    std::vector<bool> accounted(static_cast<size_t>(result->faces.Extent()), false);
    record_history(*algo, *a, 0, *result, accounted, *record);
    record_history(*algo, *b, 1, *result, accounted, *record);
    for (size_t i = 0; i < accounted.size(); ++i) {
      if (!accounted[i]) {
        record->generated.push_back(static_cast<uint32_t>(i));
      }
    }
    *history = record.release();
    return result.release();
  });
}

size_t occt_history_len(const OcctHistory* history) {
  return history == nullptr ? 0 : history->entries.size();
}

// Reads entry `index`. `results` points into the history object and is valid
// until occt_history_free. Returns false when the index is out of range.
bool occt_history_entry(const OcctHistory* history, size_t index,
                        uint32_t* source_shape, uint32_t* source_face, uint32_t* fate,
                        const uint32_t** results, size_t* result_count) {
  if (history == nullptr || index >= history->entries.size() || source_shape == nullptr ||
      source_face == nullptr || fate == nullptr || results == nullptr || result_count == nullptr) {
    return false;
  }
  const OcctHistoryEntry& entry = history->entries[index];
  *source_shape = entry.source_shape;
  *source_face = entry.source_face;
  *fate = entry.fate;
  *results = entry.results.data();
  *result_count = entry.results.size();
  return true;
}

// Output faces with no input ancestor. Valid until occt_history_free.
void occt_history_generated(const OcctHistory* history, const uint32_t** faces, size_t* count) {
  if (faces == nullptr || count == nullptr) {
    return;
  }
  *faces = history == nullptr ? nullptr : history->generated.data();
  *count = history == nullptr ? 0 : history->generated.size();
}

size_t occt_face_count(const OcctShape* shape) {
  return shape == nullptr ? 0 : static_cast<size_t>(shape->faces.Extent());
}

// Writes volume, area, and the centre of mass (x, y, z) to out[0..5].
bool occt_mass_props(const OcctShape* shape, double* out, char** error) {
  return guarded<bool>(error, false, [&]() -> bool {
    if (shape == nullptr || out == nullptr) {
      set_error(error, "null argument");
      return false;
    }
    GProp_GProps volume;
    BRepGProp::VolumeProperties(shape->shape, volume);
    GProp_GProps surface;
    BRepGProp::SurfaceProperties(shape->shape, surface);
    const gp_Pnt centre = volume.CentreOfMass();
    out[0] = volume.Mass();
    out[1] = surface.Mass();
    out[2] = centre.X();
    out[3] = centre.Y();
    out[4] = centre.Z();
    return true;
  });
}

// Triangulates a shape. Vertices are not shared between faces.
// Triangles are wound counter-clockwise seen from outside.
OcctMesh* occt_tessellate(const OcctShape* shape, double linear_deflection,
                          double angular_deflection, char** error) {
  return guarded<OcctMesh*>(error, nullptr, [&]() -> OcctMesh* {
    if (shape == nullptr) {
      set_error(error, "null argument");
      return nullptr;
    }
    // Meshing changes the shape it runs on: it stores triangulations and updates edge data.
    // Mesh a copy that shares the geometry, so the stored shape is the same before and after
    // and later operations and B-rep bytes do not depend on whether it was displayed.
    BRepBuilderAPI_Copy copier(shape->shape, Standard_False, Standard_False);
    if (!copier.IsDone()) {
      set_error(error, "OCCT: copy for meshing failed");
      return nullptr;
    }
    // Not run in parallel.
    BRepMesh_IncrementalMesh mesher(copier.Shape(), linear_deflection, Standard_False,
                                    angular_deflection, Standard_False);
    // The copy has the same structure, so its faces map in the same order and orientation.
    const OcctShape meshed(copier.Shape());
    if (meshed.faces.Extent() != shape->faces.Extent()) {
      set_error(error, "OCCT: the copy for meshing has a different face count");
      return nullptr;
    }

    std::unique_ptr<OcctMesh> mesh(new OcctMesh());
    for (int f = 1; f <= shape->faces.Extent(); ++f) {
      const TopoDS_Face face = TopoDS::Face(meshed.faces(f));
      TopLoc_Location location;
      const Handle(Poly_Triangulation)& triangulation = BRep_Tool::Triangulation(face, location);
      if (triangulation.IsNull()) {
        set_error(error, "OCCT: face " + std::to_string(f - 1) + " could not be triangulated");
        return nullptr;
      }
      const gp_Trsf transform = location.Transformation();
      const bool reversed = face.Orientation() == TopAbs_REVERSED;
      const uint32_t base = static_cast<uint32_t>(mesh->positions.size() / 3);
      const int node_count = triangulation->NbNodes();

      std::vector<gp_Pnt> points;
      points.reserve(static_cast<size_t>(node_count));
      for (int n = 1; n <= node_count; ++n) {
        points.push_back(triangulation->Node(n).Transformed(transform));
      }

      // Normals from the surface where it has one, filled in from triangles otherwise.
      std::vector<gp_Vec> normals(static_cast<size_t>(node_count), gp_Vec(0.0, 0.0, 0.0));
      std::vector<bool> has_normal(static_cast<size_t>(node_count), false);
      if (triangulation->HasUVNodes()) {
        BRepAdaptor_Surface surface(face, Standard_False);
        BRepLProp_SLProps properties(surface, 1, 1.0e-9);
        for (int n = 1; n <= node_count; ++n) {
          const gp_Pnt2d uv = triangulation->UVNode(n);
          properties.SetParameters(uv.X(), uv.Y());
          if (properties.IsNormalDefined()) {
            gp_Dir direction = properties.Normal();
            if (reversed) {
              direction.Reverse();
            }
            normals[static_cast<size_t>(n - 1)] = gp_Vec(direction);
            has_normal[static_cast<size_t>(n - 1)] = true;
          }
        }
      }

      for (int t = 1; t <= triangulation->NbTriangles(); ++t) {
        int n1 = 0;
        int n2 = 0;
        int n3 = 0;
        triangulation->Triangle(t).Get(n1, n2, n3);
        if (reversed) {
          std::swap(n2, n3);
        }
        if (n1 < 1 || n2 < 1 || n3 < 1 || n1 > node_count || n2 > node_count || n3 > node_count) {
          set_error(error, "OCCT: triangulation refers to a missing node");
          return nullptr;
        }
        const int corners[3] = {n1, n2, n3};
        gp_Vec flat = gp_Vec(points[static_cast<size_t>(n1 - 1)], points[static_cast<size_t>(n2 - 1)])
                          .Crossed(gp_Vec(points[static_cast<size_t>(n1 - 1)],
                                          points[static_cast<size_t>(n3 - 1)]));
        for (int corner : corners) {
          const size_t node = static_cast<size_t>(corner - 1);
          if (!has_normal[node] && flat.Magnitude() > 0.0) {
            normals[node] = flat.Normalized();
            has_normal[node] = true;
          }
          mesh->triangles.push_back(base + static_cast<uint32_t>(node));
        }
        mesh->triangle_faces.push_back(static_cast<uint32_t>(f - 1));
      }

      for (int n = 0; n < node_count; ++n) {
        const gp_Pnt& p = points[static_cast<size_t>(n)];
        const gp_Vec& v = normals[static_cast<size_t>(n)];
        mesh->positions.insert(mesh->positions.end(), {p.X(), p.Y(), p.Z()});
        mesh->normals.insert(mesh->normals.end(), {v.X(), v.Y(), v.Z()});
      }
    }
    return mesh.release();
  });
}

// Array views into a mesh, valid until occt_mesh_free.
void occt_mesh_data(const OcctMesh* mesh,
                    const double** positions, const double** normals, size_t* vertex_count,
                    const uint32_t** triangles, const uint32_t** triangle_faces,
                    size_t* triangle_count) {
  if (mesh == nullptr || positions == nullptr || normals == nullptr || vertex_count == nullptr ||
      triangles == nullptr || triangle_faces == nullptr || triangle_count == nullptr) {
    return;
  }
  *positions = mesh->positions.data();
  *normals = mesh->normals.data();
  *vertex_count = mesh->positions.size() / 3;
  *triangles = mesh->triangles.data();
  *triangle_faces = mesh->triangle_faces.data();
  *triangle_count = mesh->triangle_faces.size();
}

}  // extern "C"
