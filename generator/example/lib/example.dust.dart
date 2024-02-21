// GENERATED CODE - DO NOT MODIFY BY HAND

part of 'example.dart';

// **************************************************************************
// DustGenerator
// **************************************************************************

mixin _$Todo {
  $TodoRepository get $repository => const $TodoRepository();

  Atom<String> get title$ => throw UnimplementedError();
  String get title => title$.get(null);
  set title(String value) => title$.set(value);

  AtomOption<String> get description$ => throw UnimplementedError();
  String? get description => description$.get(null);
  set description(String? value) => description$.set(value);

  Atom<bool> get completed$ => throw UnimplementedError();
  bool get completed => completed$.get(null);
  set completed(bool value) => completed$.set(value);
}

class _Todo extends Todo {
  _Todo._(this.title$, this.description$, this.completed$) : super._();

  factory _Todo({
    required String title,
    String? description,
    bool completed = false,
  }) =>
      $TodoRepository().create() as _Todo;

  @override
  final Atom<String> title$;

  @override
  final AtomOption<String> description$;

  @override
  final Atom<bool> completed$;
}

class $TodoRepository implements Repository<Todo> {
  const $TodoRepository();

  static bool $init = false;

  Todo create() {
    final id = Dust.instance.randomId();
    final node = get(id);
    $write(id);
    return node.get(null)!;
  }

  void $write(Id id) {}

  @override
  void delete(Todo model) {
    // TODO: implement delete
  }

  @override
  NodeOption<Todo> get(Id id) {
    // TODO: implement get
    throw UnimplementedError();
  }

  @override
  Id id(Todo model) {
    // TODO: implement id
    throw UnimplementedError();
  }

  @override
  Schema init() {
    $init = true;
    return const Schema(
      stickyNodes: [],
      stickyAtoms: [],
      stickyEdges: [],
      acyclicEdges: [],
    );
  }
}